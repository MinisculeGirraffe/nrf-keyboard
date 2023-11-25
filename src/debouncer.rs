pub struct Debouncer {
    counter: i8,
    state: DebounceState,
    thres_steady: i8,
    thres_transient: i8,
    thres_transient_abs: i8,
}

enum DebounceState {
    SteadyStateLow,
    TransientLowHigh,
    SteadyStateHigh,
    TransientHighLow,
}

impl Debouncer {
    pub fn new(initial_value: bool, thres_transient: i8, thres_steady: i8) -> Self {
        assert!(thres_steady > thres_transient);

        let state = if initial_value {
            DebounceState::SteadyStateHigh
        } else {
            DebounceState::SteadyStateLow
        };

        let counter = if initial_value {
            thres_steady
        } else {
            -thres_steady
        };

        Debouncer {
            counter,
            state,
            thres_steady,
            thres_transient,
            thres_transient_abs: thres_steady - thres_transient,
        }
    }

    pub fn output(&self) -> bool {
        matches!(
            self.state,
            DebounceState::TransientLowHigh | DebounceState::SteadyStateHigh
        )
    }

    pub fn update(&mut self, input: bool) -> bool {
        if input {
            if self.counter < self.thres_steady {
                self.counter = self.counter + 1;
            }
        } else if self.counter > -self.thres_steady {
            self.counter = self.counter - 1;
        }

        match self.state {
            DebounceState::SteadyStateLow => self.transition_on_threshold(
                -self.thres_transient_abs,
                DebounceState::TransientLowHigh,
            ),
            DebounceState::TransientLowHigh => self.check_transition(
                self.thres_steady,
                DebounceState::SteadyStateHigh,
                -self.thres_steady,
                DebounceState::SteadyStateLow,
            ),
            DebounceState::SteadyStateHigh => self
                .transition_on_threshold(self.thres_transient_abs, DebounceState::TransientHighLow),
            DebounceState::TransientHighLow => self.check_transition(
                self.thres_steady,
                DebounceState::SteadyStateHigh,
                -self.thres_steady,
                DebounceState::SteadyStateLow,
            ),
        }
    }

    fn transition_on_threshold(&mut self, threshold: i8, next_state: DebounceState) -> bool {
        if self.counter >= threshold {
            self.counter = 0;
            self.state = next_state;
            true
        } else {
            false
        }
    }

    fn check_transition(
        &mut self,
        high_threshold: i8,
        high_state: DebounceState,
        low_threshold: i8,
        low_state: DebounceState,
    ) -> bool {
        match self.counter {
            c if c == high_threshold => {
                self.state = high_state;
                false
            }
            c if c == low_threshold => {
                self.state = low_state;
                true
            }
            _ => false,
        }
    }
}
