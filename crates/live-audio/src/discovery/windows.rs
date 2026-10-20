use super::*;
use ::windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS},
        Media::Audio::{
            DEVICE_STATE, DEVICE_STATEMASK_ALL, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
            eCapture, eRender,
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
    core::{GUID, PCWSTR, PWSTR, w},
};

struct ComApartment;
impl Drop for ComApartment {
    fn drop(&mut self) {
        // Constructed only after successful initialization on this same thread.
        unsafe { CoUninitialize() };
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
