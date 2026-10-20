//! Device capability and output route validation.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bus {
    Master,
    Booth,
    Cue,
    Record,
    Stream,
    DeckAPre,
    DeckAPost,
    DeckBPre,
    DeckBPost,
}

/// Zero-based stereo source and hardware destination channel indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutputRoute {
    pub bus: Bus,
    pub source_channel: u8,
    pub device_channel: u16,
}

/// An owned control-thread configuration, not a running audio graph.
#[derive(Debug)]
pub struct ValidatedRoutes {
    sample_rate: u32,
    routes: Vec<OutputRoute>,
}

impl ValidatedRoutes {
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn routes(&self) -> &[OutputRoute] {
        &self.routes
    }
}

/// Compile on the control thread. Empty routes mean silence; distinct edges
/// sharing an output intentionally sum. This does not publish to a callback.
pub fn compile_routes(
    capabilities: &DeviceCapabilities,
    sample_rate: u32,
    routes: &[OutputRoute],
) -> Result<ValidatedRoutes, RouteError> {
    validate_device(capabilities, sample_rate)?;
    let mut seen = std::collections::HashSet::new();
    for route in routes {
        if route.source_channel >= 2 || route.device_channel >= capabilities.output_channels {
            return Err(RouteError::ChannelOutOfRange);
        }
        if !seen.insert(*route) {
            return Err(RouteError::DuplicateRoute);
        }
    }
    Ok(ValidatedRoutes {
        sample_rate,
        routes: routes.to_vec(),
    })
}

#[derive(Debug)]
pub struct DeviceCapabilities {
    pub sample_rates: Vec<u32>,
    pub output_channels: u16,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RouteError {
    UnsupportedSampleRate,
    NoOutputChannels,
    ChannelOutOfRange,
    DuplicateRoute,
}

pub fn validate_device(
    capabilities: &DeviceCapabilities,
    sample_rate: u32,
) -> Result<(), RouteError> {
    if ![44_100, 48_000, 96_000].contains(&sample_rate)
        || !capabilities.sample_rates.contains(&sample_rate)
    {
        return Err(RouteError::UnsupportedSampleRate);
    }
    if capabilities.output_channels == 0 {
        return Err(RouteError::NoOutputChannels);
    }
    Ok(())
}
