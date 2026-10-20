//! Device capability and output route validation.

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
