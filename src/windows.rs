//! Windows file access and browser launch through fixed operating-system APIs.
use anyhow::{Context, ensure};
use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
        },
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, GetTokenInformation,
        PROTECTED_DACL_SECURITY_INFORMATION, TOKEN_QUERY, TOKEN_USER, TokenUser,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
    UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: this handle was returned by OpenProcessToken and is owned here.
        unsafe { CloseHandle(self.0) };
    }
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        // SAFETY: the SID/descriptor conversion APIs allocate with LocalAlloc.
        unsafe { LocalFree(self.0) };
    }
}
fn win32_success(result: i32) -> anyhow::Result<()> {
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

fn current_sid() -> anyhow::Result<String> {
    // SAFETY: pointers passed to these APIs refer to live, aligned buffers.
    // Each owned handle and API-allocated buffer is released exactly once.
    unsafe {
        let mut handle = ptr::null_mut();
        win32_success(OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_QUERY,
            &mut handle,
        ))?;
        let token = Token(handle);
        let mut size = 0;
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut size);
        ensure!(
            size >= size_of::<TOKEN_USER>() as u32,
            "cannot query current Windows user"
        );
        let mut buffer = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
        win32_success(GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            size,
            &mut size,
        ))?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = ptr::null_mut();
        win32_success(ConvertSidToStringSidW(user.User.Sid, &mut text))?;
        let owned = Local(text.cast());
        let mut length = 0;
        while *text.add(length) != 0 {
            length += 1;
        }
        let sid = String::from_utf16(std::slice::from_raw_parts(text, length))?;
        drop(owned);
        Ok(sid)
    }
}

pub fn restrict_path(path: &Path, directory: bool) -> anyhow::Result<()> {
    let inheritance = if directory { "OICI" } else { "" };
    let sid = current_sid()?;
    // Keep only the current user, Administrators and SYSTEM; child files inherit
    // this policy. A shared or portable directory must not inherit Users/Everyone.
    let sddl = format!(
        "D:P(A;{inheritance};FA;;;SY)(A;{inheritance};FA;;;BA)(A;{inheritance};FA;;;{sid})"
    );
    let descriptor: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    ensure!(!name[..name.len() - 1].contains(&0), "invalid private path");
    // SAFETY: the descriptor remains allocated while its DACL is passed to the
    // synchronous SetNamedSecurityInfoW call; all pointers are valid for the call.
    unsafe {
        let mut security = ptr::null_mut();
        win32_success(ConvertStringSecurityDescriptorToSecurityDescriptorW(
            descriptor.as_ptr(),
            SDDL_REVISION_1,
            &mut security,
            ptr::null_mut(),
        ))?;
        let owned = Local(security);
        let mut dacl = ptr::null_mut();
        let (mut present, mut defaulted) = (0, 0);
        win32_success(GetSecurityDescriptorDacl(
            security,
            &mut present,
            &mut dacl,
            &mut defaulted,
        ))?;
        ensure!(
            present != 0 && !dacl.is_null(),
            "private Windows DACL is unavailable"
        );
        let result = SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        );
        drop(owned);
        if result != 0 {
            return Err(std::io::Error::from_raw_os_error(result as i32))
                .context("cannot protect private files; use a filesystem supporting Windows ACLs");
        }
    }
    Ok(())
}

pub fn open_browser(origin: &str) -> anyhow::Result<()> {
    let verb: Vec<u16> = "open".encode_utf16().chain(Some(0)).collect();
    let url: Vec<u16> = origin.encode_utf16().chain(Some(0)).collect();
    // SAFETY: canonical HTTP(S) URL and fixed verb are NUL-terminated and remain
    // valid for the call. No shell command or user-controlled arguments are built.
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            verb.as_ptr(),
            url.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    ensure!(result as usize > 32, "cannot open the default browser");
    Ok(())
}
