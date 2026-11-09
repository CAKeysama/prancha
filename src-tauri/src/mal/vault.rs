//! Onde os tokens do MyAnimeList ficam guardados. Nunca em texto: no Windows, cifrados pela DPAPI do sistema,
//! presa à conta do Windows de quem usa o Prancha. Outro usuário do computador, ou uma cópia do banco levada para
//! outra máquina, não consegue abrir. Sem um cofre do sistema, a conexão não é guardada.

use crate::error::{AppError, AppResult};

pub trait Vault: Send + Sync {
    fn seal(&self, plain: &[u8]) -> AppResult<Vec<u8>>;
    fn open(&self, sealed: &[u8]) -> AppResult<Vec<u8>>;
}

fn unavailable() -> AppError {
    AppError::user("mal_vault", "Não foi possível usar o cofre do sistema para guardar a conexão com o MyAnimeList.")
}

/// O cofre do sistema em que o app está rodando.
pub fn system() -> Box<dyn Vault> {
    #[cfg(windows)]
    {
        Box::new(Dpapi)
    }
    #[cfg(not(windows))]
    {
        Box::new(Unavailable)
    }
}

#[cfg(not(windows))]
struct Unavailable;

#[cfg(not(windows))]
impl Vault for Unavailable {
    fn seal(&self, _plain: &[u8]) -> AppResult<Vec<u8>> {
        Err(AppError::user("mal_vault", "Este sistema não tem um cofre compatível para guardar a conexão com o MyAnimeList."))
    }
    fn open(&self, _sealed: &[u8]) -> AppResult<Vec<u8>> {
        Err(unavailable())
    }
}

#[cfg(windows)]
struct Dpapi;

#[cfg(windows)]
impl Vault for Dpapi {
    fn seal(&self, plain: &[u8]) -> AppResult<Vec<u8>> {
        dpapi::run(plain, true).ok_or_else(unavailable)
    }
    fn open(&self, sealed: &[u8]) -> AppResult<Vec<u8>> {
        dpapi::run(sealed, false).ok_or_else(unavailable)
    }
}

#[cfg(windows)]
mod dpapi {
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};

    /// Entropia extra: outro programa da mesma conta que use a DPAPI sem ela não abre estes dados.
    const ENTROPY: &[u8] = b"Prancha/MyAnimeList/tokens";

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 }
    }

    pub fn run(data: &[u8], seal: bool) -> Option<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: os ponteiros apontam para fatias vivas durante a chamada; a saída é alocada pelo sistema e liberada abaixo.
        let ok = unsafe {
            if seal {
                CryptProtectData(&input, std::ptr::null(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            } else {
                CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            }
        };
        if ok == 0 || out.pbData.is_null() {
            return None;
        }
        // SAFETY: o sistema devolveu `cbData` bytes em `pbData`.
        let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
        // SAFETY: memória alocada pela DPAPI com LocalAlloc. Zerada antes de devolver, porque pode ser o texto aberto.
        unsafe {
            std::ptr::write_bytes(out.pbData, 0, out.cbData as usize);
            LocalFree(out.pbData as _);
        }
        Some(bytes)
    }
}

/// Cofre de teste: embaralha com uma chave fixa, só para provar que nada vai em texto ao banco.
#[cfg(test)]
pub struct TestVault;

#[cfg(test)]
impl Vault for TestVault {
    fn seal(&self, plain: &[u8]) -> AppResult<Vec<u8>> {
        let mut out = b"TV1".to_vec();
        out.extend(plain.iter().enumerate().map(|(i, b)| b ^ (0xA5u8.wrapping_add(i as u8))));
        Ok(out)
    }
    fn open(&self, sealed: &[u8]) -> AppResult<Vec<u8>> {
        let body = sealed.strip_prefix(b"TV1").ok_or_else(unavailable)?;
        Ok(body.iter().enumerate().map(|(i, b)| b ^ (0xA5u8.wrapping_add(i as u8))).collect())
    }
}
