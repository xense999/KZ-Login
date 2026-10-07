//! DPAPI (CurrentUser scope): what it protects is useless on another machine or
//! under another Windows account. Shared by every module that keeps a secret
//! on disk.

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{LocalFree, HLOCAL};
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    pub fn protect(plain: &[u8]) -> Result<Vec<u8>, String> {
        run(plain, |input, output| unsafe {
            CryptProtectData(input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, output)
        })
        .map_err(|e| format!("加密失敗：{e}"))
    }

    pub fn unprotect(cipher: &[u8]) -> Result<Vec<u8>, String> {
        run(cipher, |input, output| unsafe {
            CryptUnprotectData(input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, output)
        })
        .map_err(|e| format!("解密失敗：{e}"))
    }

    fn run(
        data: &[u8],
        call: impl FnOnce(*const CRYPT_INTEGER_BLOB, *mut CRYPT_INTEGER_BLOB) -> windows_core::Result<()>,
    ) -> windows_core::Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut output = CRYPT_INTEGER_BLOB::default();
        call(&input, &mut output)?;
        // The output buffer is LocalAlloc'd by the API; copy it out and free it.
        let bytes = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe { LocalFree(Some(HLOCAL(output.pbData as _))) };
        Ok(bytes)
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn protect(_plain: &[u8]) -> Result<Vec<u8>, String> {
        Err("只支援 Windows".into())
    }
    pub fn unprotect(_cipher: &[u8]) -> Result<Vec<u8>, String> {
        Err("只支援 Windows".into())
    }
}

pub use imp::{protect, unprotect};
