#![allow(dead_code)]

use std::{path::PathBuf, time::Duration};

use autodj_live_supervisor::{ChildCommand, LaunchConfig, Protection, RuntimeStore};
use tokio::time::{sleep, timeout};
use uuid::Uuid;
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
};

pub fn directory() -> PathBuf {
    std::env::temp_dir().join(format!("autodj-supervisor-test-{}", Uuid::new_v4()))
}

pub fn fixture(mode: &str, marker: &std::path::Path) -> ChildCommand {
    ChildCommand::new(PathBuf::from(env!("CARGO_BIN_EXE_supervisor-fixture")))
        .args([mode.into(), marker.as_os_str().to_owned()])
}

pub fn config() -> LaunchConfig {
    let root = directory();
    let target = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let engine = target.join("autodj-live-engine.exe");
    assert!(
        engine.is_file(),
        "build autodj-live-engine before supervisor integration tests"
    );
    LaunchConfig {
        engine: ChildCommand::new(engine),
        sidecar: fixture("sidecar", &root.with_extension("sidecar-pid")),
        runtime: RuntimeStore::new(root, Protection::Installed),
    }
}

pub fn running(pid: u32) -> bool {
    // Test-only liveness check; the implementation additionally checks creation time.
    unsafe {
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let alive = WaitForSingleObject(handle, 0) != WAIT_OBJECT_0;
        CloseHandle(handle);
        alive
    }
}

pub async fn exited(pid: u32) {
    timeout(Duration::from_secs(8), async {
        while running(pid) {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("child must exit");
}

pub fn marker_pid(path: &std::path::Path) -> u32 {
    std::fs::read_to_string(path)
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

pub fn with_public_acl(path: &std::path::Path, check: impl FnOnce()) {
    use std::{
        os::windows::ffi::OsStrExt,
        ptr::{null, null_mut},
    };
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::{
            Authorization::{
                ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
                SE_FILE_OBJECT, SetNamedSecurityInfoW,
            },
            DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
            PROTECTED_DACL_SECURITY_INFORMATION,
        },
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let sddl: Vec<u16> = "D:P(A;OICI;FA;;;WD)"
        .encode_utf16()
        .chain(Some(0))
        .collect();
    unsafe {
        let mut original = null_mut();
        let mut original_acl = null_mut();
        assert_eq!(
            GetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut original_acl,
                null_mut(),
                &mut original
            ),
            0
        );
        let mut public = null_mut();
        assert_ne!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut public,
                null_mut()
            ),
            0
        );
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = null_mut();
        assert_ne!(
            GetSecurityDescriptorDacl(public, &mut present, &mut acl, &mut defaulted),
            0
        );
        assert_eq!(
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                acl,
                null()
            ),
            0
        );
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(check));
        assert_eq!(
            SetNamedSecurityInfoW(
                path.as_ptr(),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                original_acl,
                null()
            ),
            0
        );
        LocalFree(original);
        LocalFree(public);
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    }
}

pub fn wire_credentials(path: &std::path::Path) -> (u16, autodj_live_protocol::ClientHello) {
    use autodj_live_protocol::{BearerToken, ClientHello, ProtocolVersion};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
        },
    };
    let record: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let encrypted: Vec<u8> = serde_json::from_value(record["protected_token"].clone()).unwrap();
    assert!(
        encrypted.len() > 32,
        "installed records must contain a DPAPI blob"
    );
    let input = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    let token = unsafe {
        assert_ne!(
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output
            ),
            0
        );
        assert_eq!(output.cbData, 32);
        let token = BearerToken::new(
            std::slice::from_raw_parts(output.pbData, 32)
                .try_into()
                .unwrap(),
        );
        LocalFree(output.pbData.cast());
        token
    };
    (
        record["port"].as_u64().unwrap() as u16,
        ClientHello {
            version: ProtocolVersion::CURRENT,
            token,
            session_id: Uuid::parse_str(record["session_id"].as_str().unwrap()).unwrap(),
        },
    )
}
