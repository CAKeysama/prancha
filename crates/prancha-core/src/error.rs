//! Erros do aplicativo. Toda mensagem que chega à interface está em português.

use serde::ser::SerializeStruct;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Mensagem já pronta para o usuário.
    #[error("{message}")]
    User { code: &'static str, message: String },
    #[error("O item não foi encontrado na biblioteca.")]
    NotFound,
    #[error("Erro no banco de dados: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("{}", io_message(.0))]
    Io(#[from] std::io::Error),
}

impl AppError {
    pub fn user(code: &'static str, message: impl Into<String>) -> Self {
        AppError::User { code, message: message.into() }
    }

    pub fn code(&self) -> &'static str {
        match self {
            AppError::User { code, .. } => code,
            AppError::NotFound => "not_found",
            AppError::Db(_) => "database",
            AppError::Io(e) => match e.kind() {
                std::io::ErrorKind::NotFound => "file_missing",
                std::io::ErrorKind::PermissionDenied => "permission_denied",
                _ => "io",
            },
        }
    }
}

pub fn io_message(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "O arquivo não foi encontrado. Ele pode ter sido movido ou excluído.".into(),
        std::io::ErrorKind::PermissionDenied => {
            "Sem permissão para ler o arquivo. Verifique as permissões ou se ele está aberto em outro programa.".into()
        }
        _ => format!("Falha ao acessar o arquivo: {e}"),
    }
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
