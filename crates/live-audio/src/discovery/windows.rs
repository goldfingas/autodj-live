use super::*;
use ::windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, S_OK},
        Media::Audio::{
            AUDCLNT_E_UNSUPPORTED_FORMAT, AUDCLNT_SHAREMODE_EXCLUSIVE, DEVICE_STATE,
            DEVICE_STATE_ACTIVE, DEVICE_STATEMASK_ALL, IAudioClient, IAudioClient3, IMMDevice,
            IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eCapture, eRender,
        },
        System::{
            Com::{
                CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
                CoTaskMemFree, CoUninitialize, STGM_READ,
                StructuredStorage::PropVariantToStringAlloc,
            },
            Registry::*,
        },
    },
    core::{GUID, HRESULT, Interface, PCWSTR, PWSTR, w},
};

struct ComApartment;
impl Drop for ComApartment {
    fn drop(&mut self) {
        // Constructed only after successful initialization on this same thread.
        unsafe { CoUninitialize() };
    }
}

struct TaskMemory(*mut std::ffi::c_void);
impl Drop for TaskMemory {
    fn drop(&mut self) {
        unsafe { CoTaskMemFree(Some(self.0)) };
    }
}

fn validated_mix_format(format: WAVEFORMATEX) -> Result<(u16, u32, u16), &'static str> {
    let tag = format.wFormatTag;
    let channels = format.nChannels;
    let sample_rate = format.nSamplesPerSec;
    let bits = format.wBitsPerSample;
    let block_align = format.nBlockAlign;
    let byte_rate = format.nAvgBytesPerSec;
    let extension_size = format.cbSize;
    if !matches!(tag, 1 | 3 | 0xfffe)
        || (tag == 0xfffe && extension_size < 22)
        || (tag != 0xfffe && extension_size != 0)
        || channels == 0
        || sample_rate == 0
        || bits == 0
        || !bits.is_multiple_of(8)
    {
        return Err("WASAPI returned an invalid shared mix format");
    }
    let expected_block = u32::from(channels)
        .checked_mul(u32::from(bits) / 8)
        .ok_or("WASAPI returned an invalid shared mix format")?;
    let expected_rate = sample_rate
        .checked_mul(expected_block)
        .ok_or("WASAPI returned an invalid shared mix format")?;
    if expected_block != u32::from(block_align) || expected_rate != byte_rate {
        return Err("WASAPI returned an invalid shared mix format");
    }
    Ok((channels, sample_rate, bits))
}

fn exclusive_format_support(result: HRESULT) -> Result<bool, &'static str> {
    if result == S_OK {
        Ok(true)
    } else if result == AUDCLNT_E_UNSUPPORTED_FORMAT {
        Ok(false)
    } else {
        Err("could not check WASAPI exclusive format")
    }
}

struct OwnedWaveFormat {
    storage: Vec<usize>,
}

impl OwnedWaveFormat {
    unsafe fn copy_with_sample_rate(
        source: *const WAVEFORMATEX,
        sample_rate_hz: u32,
    ) -> Result<Self, &'static str> {
        let header = unsafe { source.read_unaligned() };
        let byte_rate = sample_rate_hz
            .checked_mul(u32::from(header.nBlockAlign))
            .ok_or("WASAPI candidate byte rate overflowed")?;
        let bytes = std::mem::size_of::<WAVEFORMATEX>()
            .checked_add(usize::from(header.cbSize))
            .ok_or("WASAPI candidate size overflowed")?;
        let words = bytes.div_ceil(std::mem::size_of::<usize>());
        let mut storage = vec![0usize; words];
        unsafe {
            std::ptr::copy_nonoverlapping(source.cast::<u8>(), storage.as_mut_ptr().cast(), bytes);
        }
        let candidate = storage.as_mut_ptr().cast::<WAVEFORMATEX>();
        let mut header = unsafe { candidate.read_unaligned() };
        header.nSamplesPerSec = sample_rate_hz;
        header.nAvgBytesPerSec = byte_rate;
        unsafe { candidate.write_unaligned(header) };
        Ok(Self { storage })
    }

    fn as_ptr(&self) -> *const WAVEFORMATEX {
        self.storage.as_ptr().cast()
    }
}

struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// Owns only strings allocated by COM's task allocator.
struct ComString(PWSTR);
impl ComString {
    fn into_string(self) -> Result<String, String> {
        unsafe { self.0.to_string().map_err(|error| error.to_string()) }
    }
}
impl Drop for ComString {
    fn drop(&mut self) {
        unsafe {
            CoTaskMemFree(Some(self.0.0.cast()));
        }
    }
}

pub(super) fn discover() -> Discovery {
    let mut result = Discovery::default();
    // The public entry point gives this operation its own thread/apartment.
    match unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() } {
        Ok(()) => {
            let _apartment = ComApartment;
            if let Err(error) = endpoints(&mut result) {
                result
                    .issues
                    .push(format!("Windows endpoint enumeration: {error}"));
            }
        }
        Err(error) => result.issues.push(format!("COM initialization: {error}")),
    }
    if let Err(error) = asio(&mut result) {
        result
            .issues
            .push(format!("64-bit ASIO registration enumeration: {error}"));
    }
    result
}

pub(super) fn probe_wasapi(endpoint_id: &str) -> Result<WasapiCapabilities, &'static str> {
    unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok() }
        .map_err(|_| "could not initialize WASAPI probe")?;
    let _apartment = ComApartment;
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
            .map_err(|_| "could not create WASAPI endpoint enumerator")?;
    let wide: Vec<u16> = endpoint_id.encode_utf16().chain(Some(0)).collect();
    let device = unsafe { enumerator.GetDevice(PCWSTR(wide.as_ptr())) }
        .map_err(|_| "WASAPI endpoint is unavailable")?;
    if unsafe { device.GetState() }.map_err(|_| "could not read WASAPI endpoint state")?
        != DEVICE_STATE_ACTIVE
    {
        return Err("WASAPI endpoint is not active");
    }
    let client: IAudioClient = unsafe { device.Activate(CLSCTX_INPROC_SERVER, None) }
        .map_err(|_| "could not activate WASAPI endpoint")?;
    let format_ptr =
        unsafe { client.GetMixFormat() }.map_err(|_| "could not read WASAPI shared mix format")?;
    if format_ptr.is_null() {
        return Err("WASAPI returned no shared mix format");
    }
    let _format_memory = TaskMemory(format_ptr.cast());
    let format = unsafe { format_ptr.read_unaligned() };
    let (channels, sample_rate, bits) = validated_mix_format(format)?;
    let client3: IAudioClient3 = client
        .cast()
        .map_err(|_| "WASAPI shared engine periods are unavailable")?;
    let mut shared_default_frames = 0;
    let mut shared_fundamental_frames = 0;
    let mut shared_minimum_frames = 0;
    let mut shared_maximum_frames = 0;
    unsafe {
        client3.GetSharedModeEnginePeriod(
            format_ptr,
            &mut shared_default_frames,
            &mut shared_fundamental_frames,
            &mut shared_minimum_frames,
            &mut shared_maximum_frames,
        )
    }
    .map_err(|_| "could not read WASAPI shared engine periods")?;
    let shared_mode_engine_periods = shared_mode_engine_periods(
        shared_default_frames,
        shared_fundamental_frames,
        shared_minimum_frames,
        shared_maximum_frames,
    )?;
    let exclusive_shared_mix_supported = exclusive_format_support(unsafe {
        client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, format_ptr, None)
    })?;
    let mut exclusive_shared_format_sample_rates_hz = Vec::new();
    for candidate_rate in OFFICIAL_SAMPLE_RATES_HZ {
        let supported = if candidate_rate == sample_rate {
            exclusive_shared_mix_supported
        } else {
            let candidate =
                unsafe { OwnedWaveFormat::copy_with_sample_rate(format_ptr, candidate_rate) }?;
            exclusive_format_support(unsafe {
                client.IsFormatSupported(AUDCLNT_SHAREMODE_EXCLUSIVE, candidate.as_ptr(), None)
            })?
        };
        if supported {
            exclusive_shared_format_sample_rates_hz.push(candidate_rate);
        }
    }
    let mut default_period = 0;
    let mut minimum_period = 0;
    unsafe { client.GetDevicePeriod(Some(&mut default_period), Some(&mut minimum_period)) }
        .map_err(|_| "could not read WASAPI engine periods")?;
    wasapi_capabilities(
        endpoint_id.to_owned(),
        channels,
        sample_rate,
        bits,
        (default_period, minimum_period, shared_mode_engine_periods),
        exclusive_shared_mix_supported,
        exclusive_shared_format_sample_rates_hz,
    )
}

fn endpoints(result: &mut Discovery) -> ::windows::core::Result<()> {
    // MMDeviceEnumerator only enumerates metadata. Never activate IAudioClient.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)? };
    for (flow, direction) in [(eCapture, Direction::Input), (eRender, Direction::Output)] {
        let collection = match unsafe {
            enumerator.EnumAudioEndpoints(flow, DEVICE_STATE(DEVICE_STATEMASK_ALL))
        } {
            Ok(collection) => collection,
            Err(error) => {
                result
                    .issues
                    .push(format!("{direction:?} endpoints: {error}"));
                continue;
            }
        };
        let count = match unsafe { collection.GetCount() } {
            Ok(count) => count,
            Err(error) => {
                result
                    .issues
                    .push(format!("{direction:?} endpoint count: {error}"));
                continue;
            }
        };
        for index in 0..count {
            let entry = unsafe { collection.Item(index) }
                .map_err(|error| error.to_string())
                .and_then(|device| endpoint(&device, direction, &mut result.issues));
            result.record_endpoint(entry, direction, index);
        }
    }
    Ok(())
}

fn endpoint(
    device: &IMMDevice,
    direction: Direction,
    issues: &mut Vec<String>,
) -> Result<Endpoint, String> {
    let id = ComString(unsafe { device.GetId() }.map_err(|e| e.to_string())?).into_string()?;
    let state = unsafe { device.GetState() }.map_err(|e| e.to_string())?;
    let name = (|| -> Result<String, String> {
        let properties =
            unsafe { device.OpenPropertyStore(STGM_READ) }.map_err(|e| e.to_string())?;
        let value =
            unsafe { properties.GetValue(&PKEY_Device_FriendlyName) }.map_err(|e| e.to_string())?;
        ComString(unsafe { PropVariantToStringAlloc(&value) }.map_err(|e| e.to_string())?)
            .into_string()
    })();
    let name = endpoint_label(&id, name, issues);
    Ok(Endpoint {
        id,
        name,
        direction,
        state: endpoint_state(state.0),
    })
}

fn asio(result: &mut Discovery) -> Result<(), String> {
    let mut raw = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            w!("SOFTWARE\\ASIO"),
            None,
            KEY_READ | KEY_WOW64_64KEY,
            &mut raw,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    status.ok().map_err(|e| e.to_string())?;
    let key = Key(raw);
    for index in 0.. {
        // Windows registry key names are limited to 255 UTF-16 code units.
        let mut name = [0u16; 256];
        let mut length = name.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                Some(PWSTR(name.as_mut_ptr())),
                &mut length,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        status.ok().map_err(|e| e.to_string())?;
        let registration = match String::from_utf16(&name[..length as usize]) {
            Ok(name) => name,
            Err(error) => {
                result
                    .issues
                    .push(format!("ASIO registration {index} name: {error}"));
                continue;
            }
        };
        match asio_clsid(key.0, PCWSTR(name.as_ptr())) {
            Ok(clsid) => result.asio_drivers.push(AsioRegistration {
                registration,
                clsid,
            }),
            Err(error) => result.issues.push(format!("ASIO {registration}: {error}")),
        }
    }
    Ok(())
}

fn asio_clsid(key: HKEY, subkey: PCWSTR) -> Result<String, String> {
    // RegGetValueW may supply a missing REG_SZ terminator. Accept that documented
    // normalization; validate the returned text and GUID, not raw registry bytes.
    // A CLSID is 38 characters with braces; oversized values are malformed.
    let mut value = [0u16; 64];
    let mut bytes = std::mem::size_of_val(&value) as u32;
    unsafe {
        RegGetValueW(
            key,
            subkey,
            w!("CLSID"),
            RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut bytes),
        )
    }
    .ok()
    .map_err(|e| e.to_string())?;
    if !bytes.is_multiple_of(2) || bytes as usize > std::mem::size_of_val(&value) {
        return Err("Invalid CLSID byte length".into());
    }
    parse_clsid(&value[..bytes as usize / 2])
}

fn parse_clsid(value: &[u16]) -> Result<String, String> {
    let text = registry_text(value)?;
    // Parse without CoCreateInstance: enumeration must never load ASIO drivers.
    let unbraced = text
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(&text);
    GUID::try_from(unbraced).map_err(|e| e.to_string())?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wave_format(
        tag: u16,
        channels: u16,
        rate: u32,
        bits: u16,
        cb_size: u16,
    ) -> ::windows::Win32::Media::Audio::WAVEFORMATEX {
        let block_align = channels * (bits / 8);
        ::windows::Win32::Media::Audio::WAVEFORMATEX {
            wFormatTag: tag,
            nChannels: channels,
            nSamplesPerSec: rate,
            nAvgBytesPerSec: rate * u32::from(block_align),
            nBlockAlign: block_align,
            wBitsPerSample: bits,
            cbSize: cb_size,
        }
    }

    #[test]
    fn native_mix_format_must_be_coherent_before_fields_are_exposed() {
        assert_eq!(
            validated_mix_format(wave_format(0xfffe, 8, 48_000, 32, 22)).unwrap(),
            (8, 48_000, 32)
        );
        assert_eq!(
            validated_mix_format(wave_format(3, 2, 48_000, 32, 0)).unwrap(),
            (2, 48_000, 32)
        );

        let mut invalid = [
            wave_format(3, 2, 48_000, 7, 0),
            wave_format(3, 2, 48_000, 32, 0),
            wave_format(3, 2, 48_000, 32, 0),
            wave_format(0xfffe, 2, 48_000, 32, 21),
            wave_format(0x1234, 2, 48_000, 32, 0),
        ];
        invalid[1].nBlockAlign = 0;
        invalid[2].nAvgBytesPerSec = 1;
        for format in invalid {
            assert!(validated_mix_format(format).is_err());
        }
    }

    #[test]
    fn exclusive_format_result_distinguishes_unsupported_from_probe_failure() {
        use ::windows::{
            Win32::{Foundation::E_FAIL, Media::Audio::AUDCLNT_E_UNSUPPORTED_FORMAT},
            core::HRESULT,
        };

        assert!(exclusive_format_support(HRESULT(0)).unwrap());
        assert!(!exclusive_format_support(AUDCLNT_E_UNSUPPORTED_FORMAT).unwrap());
        assert!(exclusive_format_support(E_FAIL).is_err());
    }

    #[test]
    fn candidate_changes_only_rate_fields_and_preserves_extension_bytes() {
        use ::windows::Win32::Media::Audio::WAVEFORMATEXTENSIBLE;

        let source = WAVEFORMATEXTENSIBLE {
            Format: wave_format(0xfffe, 8, 48_000, 32, 22),
            Samples: ::windows::Win32::Media::Audio::WAVEFORMATEXTENSIBLE_0 {
                wValidBitsPerSample: 24,
            },
            dwChannelMask: 0x63f,
            SubFormat: GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71),
        };
        let source_bytes = unsafe {
            std::slice::from_raw_parts(
                (&raw const source).cast::<u8>(),
                std::mem::size_of::<WAVEFORMATEXTENSIBLE>(),
            )
        };

        let candidate =
            unsafe { OwnedWaveFormat::copy_with_sample_rate((&raw const source.Format), 96_000) }
                .unwrap();
        let header = unsafe { candidate.as_ptr().read_unaligned() };
        let sample_rate = header.nSamplesPerSec;
        let byte_rate = header.nAvgBytesPerSec;
        let candidate_bytes = unsafe {
            std::slice::from_raw_parts(candidate.as_ptr().cast::<u8>(), source_bytes.len())
        };

        assert_eq!(sample_rate, 96_000);
        assert_eq!(byte_rate, 3_072_000);
        assert_eq!(
            &candidate_bytes[std::mem::size_of::<WAVEFORMATEX>()..],
            &source_bytes[std::mem::size_of::<WAVEFORMATEX>()..]
        );
    }

    #[test]
    fn candidate_rejects_byte_rate_overflow() {
        let source = WAVEFORMATEX {
            nBlockAlign: u16::MAX,
            ..wave_format(3, 2, 48_000, 32, 0)
        };

        assert!(
            unsafe { OwnedWaveFormat::copy_with_sample_rate(&raw const source, 96_000) }.is_err()
        );
    }

    #[test]
    fn clsid_validation_rejects_malformed_registration() {
        for invalid in ["garbage", "{01234567-89AB-CDEF-0123-456789ABCDEZ}", ""] {
            let value: Vec<u16> = invalid.encode_utf16().chain(Some(0)).collect();
            assert!(parse_clsid(&value).is_err());
        }
        for valid in [
            "{01234567-89AB-CDEF-0123-456789ABCDEF}",
            "01234567-89AB-CDEF-0123-456789ABCDEF",
        ] {
            let value: Vec<u16> = valid.encode_utf16().chain(Some(0)).collect();
            assert_eq!(parse_clsid(&value).unwrap(), valid);
        }
    }
}
