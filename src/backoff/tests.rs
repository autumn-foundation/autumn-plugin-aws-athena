use std::time::Duration;

use proptest::prelude::*;

use super::Backoff;

const fn ms(v: u64) -> Duration {
    Duration::from_millis(v)
}

#[test]
fn first_delay_is_the_initial_delay() {
    assert_eq!(Backoff::new(ms(100), ms(1000), 2.0).delay(0), ms(100));
}

#[test]
fn delay_grows_by_the_multiplier() {
    let backoff = Backoff::new(ms(100), ms(10_000), 2.0);
    assert_eq!(backoff.delay(1), ms(200));
    assert_eq!(backoff.delay(3), ms(800));
}

#[test]
fn delay_stops_at_the_cap() {
    let backoff = Backoff::new(ms(100), ms(1000), 2.0);
    assert_eq!(backoff.delay(4), ms(1000));
    assert_eq!(backoff.delay(u32::MAX), ms(1000));
}

#[test]
fn multiplier_one_gives_a_constant_delay() {
    let backoff = Backoff::new(ms(250), ms(1000), 1.0);
    assert_eq!(backoff.delay(0), ms(250));
    assert_eq!(backoff.delay(50), ms(250));
}

proptest! {
    #[test]
    fn delay_is_monotonic_and_in_bounds(
        initial in 1_u64..5_000,
        extra in 0_u64..60_000,
        multiplier in 1.0_f64..10.0,
        attempt in 0_u32..10_000,
    ) {
        let backoff = Backoff::new(ms(initial), ms(initial + extra), multiplier);
        let now = backoff.delay(attempt);
        let next = backoff.delay(attempt.saturating_add(1));
        prop_assert!(now >= ms(initial));
        prop_assert!(now <= ms(initial + extra));
        prop_assert!(next >= now);
    }
}
