//! Small Windows security/identity primitives. Handles never inherit by default.

use crate::SupervisorError;
use std::{
    ffi::{OsStr, c_void},
    fs::File,
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ALREADY_EXISTS, ERROR_INVALID_PARAMETER, GetLastError, LocalFree, WAIT_OBJECT_0,
    },
    Security::{
        ACCESS_ALLOWED_ACE,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetNamedSecurityInfoW, SE_FILE_OBJECT,
        },
        CONTAINER_INHERIT_ACE,
        Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
        },
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetSecurityDescriptorControl,
        GetTokenInformation, OBJECT_INHERIT_ACE, OWNER_SECURITY_INFORMATION, SE_DACL_PROTECTED,
        SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        CreateDirectoryW, FILE_ALL_ACCESS, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        MoveFileExW,
    },
    System::Threading::{
        GetCurrentProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    },
};

pub(crate) fn wide(value: &OsStr) -> Result<Vec<u16>, SupervisorError> {
    let mut value: Vec<u16> = value.encode_wide().collect();
    if value.contains(&0) {
        return Err(SupervisorError::Spawn);
    }
    value.push(0);
    Ok(value)
}

pub(crate) struct Local(pub *mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: all instances own a LocalAlloc-family result from Windows.
        unsafe {
            LocalFree(self.0);
        }
    }
}

fn current_user() -> Result<Vec<usize>, SupervisorError> {
    // SAFETY: valid output pointers, owned token handle, aligned buffer of the
    // size returned by GetTokenInformation. TOKEN_USER remains in this buffer.
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(SupervisorError::Privacy);
        }
        let token = OwnedHandle::from_raw_handle(token);
        let mut size = 0;
        GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut size);
        let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        if GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        ) == 0
        {
            return Err(SupervisorError::Privacy);
        }
        Ok(buffer)
    }
}

pub(crate) fn private_directory(path: &Path) -> Result<(), SupervisorError> {
    let user = current_user()?;
    let name = wide(path.as_os_str())?;
    // SAFETY: TOKEN_USER, SIDs and security descriptors come from Windows; all
    // allocations and handles live through the calls that borrow them.
    unsafe {
        let sid = (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid;
        let mut sid_text = null_mut();
        if ConvertSidToStringSidW(sid, &mut sid_text) == 0 {
            return Err(SupervisorError::Privacy);
        }
        let _sid_text = Local(sid_text.cast());
        let mut length = 0;
        while *sid_text.add(length) != 0 {
            length += 1;
        }
        let sid_string = String::from_utf16(std::slice::from_raw_parts(sid_text, length))
            .map_err(|_| SupervisorError::Privacy)?;
        let sddl = wide(OsStr::new(&format!(
            "O:{sid_string}D:P(A;OICI;FA;;;{sid_string})"
        )))?;
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(SupervisorError::Privacy);
        }
        let _descriptor = Local(descriptor);
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        if CreateDirectoryW(name.as_ptr(), &attributes) == 0
            && GetLastError() != ERROR_ALREADY_EXISTS
        {
            return Err(SupervisorError::Privacy);
        }
    }
    private_file(path, true)
}

pub(crate) fn private_file(path: &Path, directory: bool) -> Result<(), SupervisorError> {
    reject_reparse(path)?;
    let metadata = std::fs::metadata(path).map_err(|_| SupervisorError::Privacy)?;
    if metadata.is_dir() != directory {
        return Err(SupervisorError::Privacy);
    }
    let user = current_user()?;
    let name = wide(path.as_os_str())?;
    // SAFETY: Windows owns the SID/security descriptor layouts. The backing
    // allocations remain live until all owner and ACE comparisons complete.
    unsafe {
        let sid = (*(user.as_ptr().cast::<TOKEN_USER>())).User.Sid;
        let mut owner = null_mut();
        let mut dacl = null_mut();
        let mut actual = null_mut();
        if GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut actual,
        ) != 0
        {
            return Err(SupervisorError::Privacy);
        }
        let _actual = Local(actual);
        let mut control = 0;
        let mut revision = 0;
        if GetSecurityDescriptorControl(actual, &mut control, &mut revision) == 0
            || (directory && control & SE_DACL_PROTECTED == 0)
            || dacl.is_null()
            || owner.is_null()
            || EqualSid(owner, sid) == 0
            || (*dacl).AceCount != 1
        {
            return Err(SupervisorError::Privacy);
        }
        let mut ace = null_mut();
        if GetAce(dacl, 0, &mut ace) == 0 {
            return Err(SupervisorError::Privacy);
        }
        let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
        let required_inheritance = (OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE) as u8;
        if ace.Header.AceType != 0 /* ACCESS_ALLOWED_ACE_TYPE */
            || ace.Mask & FILE_ALL_ACCESS != FILE_ALL_ACCESS
            || (directory
                && ace.Header.AceFlags & required_inheritance != required_inheritance)
            || EqualSid(
                (&ace.SidStart as *const u32).cast_mut().cast(),
                sid,
            ) == 0
        {
            return Err(SupervisorError::Privacy);
        }
    }
    Ok(())
}

pub(crate) fn reject_reparse(path: &Path) -> Result<(), SupervisorError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| SupervisorError::Privacy)?;
    if metadata.file_attributes() & 0x400 != 0 {
        return Err(SupervisorError::Privacy);
    }
    Ok(())
}

pub(crate) fn protect(bytes: &[u8], decrypt: bool) -> Result<Vec<u8>, SupervisorError> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes
            .len()
            .try_into()
            .map_err(|_| SupervisorError::Discovery)?,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // SAFETY: immutable input is borrowed by DPAPI; its allocated result is
    // copied before LocalFree. No prompt/UI or machine-wide protection is used.
    unsafe {
        let success = if decrypt {
            CryptUnprotectData(
                &input,
                null_mut(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptProtectData(
                &input,
                null(),
                null(),
                null(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        };
        if success == 0 {
            return Err(SupervisorError::Discovery);
        }
        let _output = Local(output.pbData.cast());
        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        if decrypt {
            std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        }
        Ok(result)
    }
}

pub(crate) fn replace(from: &Path, to: &Path) -> Result<(), SupervisorError> {
    let from = wide(from.as_os_str())?;
    let to = wide(to.as_os_str())?;
    // SAFETY: both paths are nul-terminated and owned for the call. Failure
    // leaves the destination in place; there is deliberately no remove fallback.
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(SupervisorError::Discovery);
    }
    Ok(())
}

pub(crate) fn creation_time(handle: &OwnedHandle) -> Result<u64, SupervisorError> {
    let mut created = Default::default();
    let mut exited = Default::default();
    let mut kernel = Default::default();
    let mut user = Default::default();
    // SAFETY: handle grants query rights and all output pointers are valid.
    if unsafe {
        GetProcessTimes(
            handle.as_raw_handle(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    } == 0
    {
        return Err(SupervisorError::Discovery);
    }
    Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

pub(crate) fn matching_process(
    pid: u32,
    created: u64,
) -> Result<Option<OwnedHandle>, SupervisorError> {
    // SAFETY: OpenProcess validates the untrusted PID. Invalid PID is the only
    // failure treated as stale; access denied cannot justify record removal.
    let handle = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            0,
            pid,
        )
    };
    if handle.is_null() {
        return if unsafe { GetLastError() } == ERROR_INVALID_PARAMETER {
            Ok(None)
        } else {
            Err(SupervisorError::Discovery)
        };
    }
    // SAFETY: OpenProcess returned an owned, non-null handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    if creation_time(&handle)? != created || exited(&handle) {
        return Ok(None);
    }
    Ok(Some(handle))
}

pub(crate) fn exited(handle: &OwnedHandle) -> bool {
    // SAFETY: handle grants SYNCHRONIZE; zero timeout is a nonblocking poll.
    unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) == WAIT_OBJECT_0 }
}

pub(crate) fn lock(file: &File) -> Result<(), SupervisorError> {
    file.try_lock().map_err(|_| SupervisorError::Busy)
}
