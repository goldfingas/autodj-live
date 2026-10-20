use live_audio::routing::{DeviceCapabilities, RouteError, validate_device};

#[test]
fn rates_must_be_official_and_advertised() {
    for rate in [44_100, 48_000, 96_000] {
        let caps = DeviceCapabilities {
            sample_rates: vec![rate],
            output_channels: 2,
        };
        assert_eq!(validate_device(&caps, rate), Ok(()));
        let absent = DeviceCapabilities {
            sample_rates: vec![],
            output_channels: 2,
        };
        assert_eq!(
            validate_device(&absent, rate),
            Err(RouteError::UnsupportedSampleRate)
        );
    }
    let caps = DeviceCapabilities {
        sample_rates: vec![22_050, 48_000],
        output_channels: 2,
    };
    assert_eq!(
        validate_device(&caps, 22_050),
        Err(RouteError::UnsupportedSampleRate)
    );
    assert_eq!(
        validate_device(&caps, 96_000),
        Err(RouteError::UnsupportedSampleRate)
    );
}

#[test]
fn output_device_needs_channels() {
    let caps = DeviceCapabilities {
        sample_rates: vec![48_000],
        output_channels: 0,
    };
    assert_eq!(
        validate_device(&caps, 48_000),
        Err(RouteError::NoOutputChannels)
    );
}
