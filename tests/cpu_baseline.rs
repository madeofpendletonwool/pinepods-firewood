//! Runs the C and C++ code that playback reaches. CI runs these on an emulated CPU
//! without AVX, where code built for a newer CPU dies with SIGILL.

use pinepods_firewood::audio::speed::Speed;

/// Anything but 1× goes through Signalsmith Stretch.
#[test]
fn time_stretching_runs() {
    let mut speed = Speed::new(2, 44_100, 1.5);
    let output = speed.process(vec![0.1; 4096 * 2]);
    assert!(!output.is_empty());
}

/// Every TLS connection needs random bytes from aws-lc, as when streaming an episode
/// from its feed's HTTPS URL.
#[test]
fn tls_random_runs() {
    let provider = rustls::crypto::aws_lc_rs::default_provider();
    let mut bytes = [0u8; 32];
    provider.secure_random.fill(&mut bytes).unwrap();
    assert_ne!(bytes, [0u8; 32]);
}
