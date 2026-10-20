use live_audio::routing::{Bus, OutputRoute, compile_routes};
use live_audio::routing::{DeviceCapabilities, RouteError, validate_device};

fn route(bus: Bus, source_channel: u8, device_channel: u16) -> OutputRoute {
    OutputRoute {
        bus,
        source_channel,
        device_channel,
    }
}

#[test]
fn master_and_cue_require_the_selected_hardware_channels() {
    let routes = [
        route(Bus::Master, 0, 0),
        route(Bus::Master, 1, 1),
        route(Bus::Cue, 0, 2),
        route(Bus::Cue, 1, 3),
    ];
    let mut caps = DeviceCapabilities {
        sample_rates: vec![48_000],
        output_channels: 4,
    };
    let active = compile_routes(&caps, 48_000, &routes).unwrap();
    assert_eq!(active.sample_rate(), 48_000);
    assert_eq!(active.routes(), &routes);
    caps.output_channels = 2;
    assert_eq!(
        compile_routes(&caps, 48_000, &routes).unwrap_err(),
        RouteError::ChannelOutOfRange
    );
    assert_eq!(active.routes(), &routes);
    assert_eq!(active.sample_rate(), 48_000);
}

#[test]
fn invalid_channels_and_duplicate_edges_are_rejected() {
    let caps = DeviceCapabilities {
        sample_rates: vec![48_000],
        output_channels: 2,
    };
    for invalid in [
        route(Bus::Cue, 2, 0),
        route(Bus::Cue, 0, 2),
        route(Bus::Cue, u8::MAX, u16::MAX),
    ] {
        assert_eq!(
            compile_routes(&caps, 48_000, &[invalid]).unwrap_err(),
            RouteError::ChannelOutOfRange
        );
    }
    let edge = route(Bus::Master, 0, 0);
    assert_eq!(
        compile_routes(&caps, 48_000, &[edge, route(Bus::Cue, 0, 1), edge]).unwrap_err(),
        RouteError::DuplicateRoute
    );
}

#[test]
fn intentional_summing_preserves_buses_and_owns_routes() {
    let caps = DeviceCapabilities {
        sample_rates: vec![48_000],
        output_channels: 2,
    };
    let mut routes = vec![route(Bus::Master, 0, 0), route(Bus::Cue, 0, 0)];
    let compiled = compile_routes(&caps, 48_000, &routes).unwrap();
    routes.clear();
    assert_eq!(
        compiled.routes(),
        &[route(Bus::Master, 0, 0), route(Bus::Cue, 0, 0)]
    );
    assert!(
        compile_routes(&caps, 48_000, &[])
            .unwrap()
            .routes()
            .is_empty()
    );
}

#[test]
fn compilation_revalidates_device_even_for_silence() {
    let caps = DeviceCapabilities {
        sample_rates: vec![48_000],
        output_channels: 2,
    };
    assert_eq!(
        compile_routes(&caps, 96_000, &[]).unwrap_err(),
        RouteError::UnsupportedSampleRate
    );
    let caps = DeviceCapabilities {
        output_channels: 0,
        ..caps
    };
    assert_eq!(
        compile_routes(&caps, 48_000, &[]).unwrap_err(),
        RouteError::NoOutputChannels
    );
}

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
