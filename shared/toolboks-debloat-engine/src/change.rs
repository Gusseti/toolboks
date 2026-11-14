//! The error of a system change that may have been rolled back.

/// A change that failed, in plain words.
#[derive(Debug)]
pub struct ChangeError<T> {
    pub message: String,
    /// The step to retry with Undo when the previous setting could not be restored.
    pub recovery: Option<T>,
}

impl<T> From<String> for ChangeError<T> {
    fn from(message: String) -> Self {
        Self {
            message,
            recovery: None,
        }
    }
}

impl<T> std::fmt::Display for ChangeError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

/// Undoes `step` after a failed change and words the error accordingly.
pub fn rollback<T>(
    message: String,
    step: T,
    undo: impl FnOnce(&T) -> Result<(), String>,
) -> ChangeError<T> {
    match undo(&step) {
        Ok(()) => format!("{message}. The previous setting was restored.").into(),
        Err(error) => ChangeError {
            message: format!(
                "{message}. Could not restore the previous setting ({error}). Use Undo to retry."
            ),
            recovery: Some(step),
        },
    }
}
