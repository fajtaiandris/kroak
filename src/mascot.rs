use std::time::{SystemTime, UNIX_EPOCH};

const HOME: &str = "▆▆O▆O▆▂";
const BLINK: [&str; 8] = [
    "▆▆o▆o▆▂",
    "▆▆-▆-▆▂",
    "▆▆o▆o▆▂",
    HOME,
    "▆▆o▆o▆▂",
    "▆▆-▆-▆▂",
    "▆▆o▆o▆▂",
    HOME,
];
const MOVE: [&str; 6] = [HOME, "▆▆▆O▆O▂", HOME, "▆O▆O▆▆▂", "O▆O▆▆▆▂", "▆O▆O▆▆▂"];
const WAVE: [&str; 7] = [HOME, "▆▆O▆O▆▘", "▆▆O▆O▆▞", HOME, "▆▆O▆O▆▘", "▆▆O▆O▆▞", HOME];
const CLICK: [&str; 11] = [
    HOME,
    "▆▆O▆O▆▂▂",
    "▆▆▆O▆O▂▂",
    "▆▆▆O▆O▆▂",
    "▂▆▆O▆O▆▂",
    "▂▆▆▆O▆O▆",
    "▂▆▆O▆O▆",
    "▂▆O▆O▆▆▂",
    "▆▆O▆O▆▆▂",
    "▆▆O▆O▆▂▂",
    HOME,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle { remaining: u8 },
    Blink { frame: usize },
    Moving { frame: usize },
    Returning { frame: usize },
    Waving { frame: usize },
    Click { frame: usize },
}

pub struct Mascot {
    phase: Phase,
    random_state: u64,
}

impl Mascot {
    pub fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(1, |duration| duration.as_nanos() as u64);
        Self::with_seed(seed)
    }

    fn with_seed(seed: u64) -> Self {
        let mut mascot = Self {
            phase: Phase::Idle { remaining: 0 },
            random_state: seed | 1,
        };
        mascot.phase = Phase::Idle {
            remaining: mascot.next_blink_delay(),
        };
        mascot
    }

    fn next_blink_delay(&mut self) -> u8 {
        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 7;
        self.random_state ^= self.random_state << 17;
        12 + (self.random_state % 18) as u8
    }

    pub fn start_work(&mut self) {
        self.phase = Phase::Moving { frame: 0 };
    }

    pub fn finish_work(&mut self) {
        if let Phase::Moving { frame } = self.phase {
            self.phase = Phase::Returning { frame };
        }
    }

    pub fn play_click(&mut self) {
        if matches!(self.phase, Phase::Idle { .. } | Phase::Blink { .. }) {
            self.phase = Phase::Click { frame: 0 };
        }
    }

    pub fn tick(&mut self) {
        self.phase = match self.phase {
            Phase::Idle { remaining } if remaining > 1 => Phase::Idle {
                remaining: remaining - 1,
            },
            Phase::Idle { .. } => Phase::Blink { frame: 0 },
            Phase::Blink { frame } if frame + 1 < BLINK.len() => Phase::Blink { frame: frame + 1 },
            Phase::Blink { .. } => Phase::Idle {
                remaining: self.next_blink_delay(),
            },
            Phase::Moving { frame } => Phase::Moving {
                frame: (frame + 1) % MOVE.len(),
            },
            Phase::Returning { frame: 0 } => Phase::Waving { frame: 0 },
            Phase::Returning { frame } => Phase::Returning {
                frame: (frame + 1) % MOVE.len(),
            },
            Phase::Waving { frame } if frame + 1 < WAVE.len() => Phase::Waving { frame: frame + 1 },
            Phase::Waving { .. } => Phase::Idle {
                remaining: self.next_blink_delay(),
            },
            Phase::Click { frame } if frame + 1 < CLICK.len() => Phase::Click { frame: frame + 1 },
            Phase::Click { .. } => Phase::Idle {
                remaining: self.next_blink_delay(),
            },
        };
    }

    pub fn frame(&self) -> &'static str {
        match self.phase {
            Phase::Idle { .. } => HOME,
            Phase::Blink { frame } => BLINK[frame],
            Phase::Moving { frame } | Phase::Returning { frame } => MOVE[frame],
            Phase::Waving { frame } => WAVE[frame],
            Phase::Click { frame } => CLICK[frame],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_blinks_twice_then_returns_home() {
        let mut mascot = Mascot::with_seed(7);
        assert_eq!(mascot.frame(), HOME);
        for _ in 0..30 {
            mascot.tick();
            if matches!(mascot.phase, Phase::Blink { .. }) {
                break;
            }
        }
        for expected in BLINK {
            assert_eq!(mascot.frame(), expected);
            mascot.tick();
        }
        assert!(matches!(mascot.phase, Phase::Idle { .. }));
        assert_eq!(mascot.frame(), HOME);
    }

    #[test]
    fn work_finishes_movement_then_waves_twice() {
        let mut mascot = Mascot::with_seed(7);
        mascot.start_work();
        for expected in MOVE {
            assert_eq!(mascot.frame(), expected);
            mascot.tick();
        }
        mascot.tick(); // finish from a frame away from home
        assert_eq!(mascot.frame(), MOVE[1]);
        mascot.finish_work();
        for expected in &MOVE[2..] {
            mascot.tick();
            assert_eq!(mascot.frame(), *expected);
        }
        mascot.tick();
        assert_eq!(mascot.frame(), HOME);
        mascot.tick();
        for expected in WAVE {
            assert_eq!(mascot.frame(), expected);
            mascot.tick();
        }
        assert!(matches!(mascot.phase, Phase::Idle { .. }));
        assert_eq!(mascot.frame(), HOME);
    }

    #[test]
    fn click_plays_full_sequence_then_resumes_idle_blinking() {
        let mut mascot = Mascot::with_seed(7);
        mascot.play_click();
        for expected in CLICK {
            assert_eq!(mascot.frame(), expected);
            mascot.tick();
        }
        assert!(matches!(mascot.phase, Phase::Idle { .. }));
        assert_eq!(mascot.frame(), HOME);
    }

    #[test]
    fn click_does_not_interrupt_work() {
        let mut mascot = Mascot::with_seed(7);
        mascot.start_work();
        mascot.tick();
        let moving = mascot.frame();
        mascot.play_click();
        assert_eq!(mascot.frame(), moving);
        assert!(matches!(mascot.phase, Phase::Moving { .. }));
    }
}
