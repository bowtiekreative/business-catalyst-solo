//! Deterministic button semantics independent of FFmpeg scheduling or UI paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Stamp {
    pub frame: u64,
    pub host_ms: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GestureState {
    Idle,
    Pressed { start: Stamp },
    Long { start: Stamp },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    None,
    Started { frame: u64 },
    Finished { start: u64, end: u64 },
    Cancelled,
}
pub(crate) struct Gesture {
    pub state: GestureState,
    threshold: u64,
}
impl Gesture {
    pub fn new(threshold: u64) -> Self {
        Self {
            state: GestureState::Idle,
            threshold,
        }
    }
    pub fn press(&mut self, at: Stamp) -> Action {
        match self.state {
            GestureState::Idle => {
                self.state = GestureState::Pressed { start: at };
                Action::Started { frame: at.frame }
            }
            GestureState::Pressed { .. } => Action::None,
            GestureState::Long { start } => {
                self.state = GestureState::Idle;
                finished(start, at)
            }
        }
    }
    pub fn release(&mut self, at: Stamp) -> Action {
        if let GestureState::Pressed { start } = self.state {
            if at.host_ms.saturating_sub(start.host_ms) < self.threshold {
                self.state = GestureState::Long { start };
                Action::None
            } else {
                self.state = GestureState::Idle;
                finished(start, at)
            }
        } else {
            Action::None
        }
    }
    pub fn stop(&mut self, at: Stamp) -> Action {
        let previous = std::mem::replace(&mut self.state, GestureState::Idle);
        match previous {
            GestureState::Pressed { start } | GestureState::Long { start } => finished(start, at),
            GestureState::Idle => Action::None,
        }
    }
    pub fn cancel(&mut self) -> Action {
        if self.state == GestureState::Idle {
            Action::None
        } else {
            self.state = GestureState::Idle;
            Action::Cancelled
        }
    }
    pub fn active_start(&self) -> Option<u64> {
        match self.state {
            GestureState::Idle => None,
            GestureState::Pressed { start } | GestureState::Long { start } => Some(start.frame),
        }
    }
}
fn finished(start: Stamp, end: Stamp) -> Action {
    Action::Finished {
        start: start.frame,
        end: end
            .frame
            .saturating_add(1)
            .max(start.frame.saturating_add(1)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tap_tap_and_twenty_holds_keep_press_frames_and_order_even_with_delayed_consumption() {
        let mut gesture = Gesture::new(350);
        for index in 0..20 {
            let frame = index * 30;
            assert_eq!(
                gesture.press(Stamp {
                    frame,
                    host_ms: index * 1000
                }),
                Action::Started { frame }
            );
            assert_eq!(
                gesture.release(Stamp {
                    frame: frame + 12,
                    host_ms: index * 1000 + 400
                }),
                Action::Finished {
                    start: frame,
                    end: frame + 13
                }
            );
            assert_eq!(gesture.state, GestureState::Idle);
        }
        gesture.press(Stamp {
            frame: 700,
            host_ms: 30_000,
        });
        assert_eq!(
            gesture.release(Stamp {
                frame: 701,
                host_ms: 30_050
            }),
            Action::None
        );
        assert!(matches!(gesture.state, GestureState::Long { .. }));
        assert_eq!(
            gesture.press(Stamp {
                frame: 1000,
                host_ms: 40_000
            }),
            Action::Finished {
                start: 700,
                end: 1001
            }
        );
        assert_eq!(
            gesture.release(Stamp {
                frame: 1001,
                host_ms: 40_020
            }),
            Action::None
        );
    }
    #[test]
    fn exact_threshold_is_hold_and_explicit_stop_or_cancel_never_enters_long_take() {
        let mut gesture = Gesture::new(350);
        gesture.press(Stamp {
            frame: 5,
            host_ms: 0,
        });
        assert!(matches!(
            gesture.release(Stamp {
                frame: 15,
                host_ms: 350
            }),
            Action::Finished { .. }
        ));
        gesture.press(Stamp {
            frame: 30,
            host_ms: 1000,
        });
        assert!(matches!(
            gesture.stop(Stamp {
                frame: 30,
                host_ms: 1001
            }),
            Action::Finished { start: 30, end: 31 }
        ));
        gesture.press(Stamp {
            frame: 40,
            host_ms: 2000,
        });
        assert_eq!(gesture.cancel(), Action::Cancelled);
        assert_eq!(
            gesture.release(Stamp {
                frame: 41,
                host_ms: 2001
            }),
            Action::None
        );
    }
}
