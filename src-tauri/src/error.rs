use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AppError {
    pub message: String,
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError {
            message: e.to_string(),
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError {
            message: e.to_string(),
        }
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError { message: s }
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError {
            message: s.to_string(),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_returns_message() {
        let err = AppError {
            message: "something went wrong".into(),
        };
        assert_eq!(format!("{}", err), "something went wrong");
    }

    #[test]
    fn from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let app_err: AppError = io_err.into();
        assert_eq!(app_err.message, "file not found");
    }

    #[test]
    fn from_serde_json_error() {
        let json_err = serde_json::from_str::<()>("invalid").unwrap_err();
        let app_err: AppError = json_err.into();
        assert!(!app_err.message.is_empty(), "message should not be empty");
        assert!(
            app_err.message.contains("at line"),
            "message should contain the error details"
        );
    }

    #[test]
    fn from_string() {
        let app_err: AppError = "custom string error".to_string().into();
        assert_eq!(app_err.message, "custom string error");
    }

    #[test]
    fn from_str() {
        let app_err: AppError = "a str slice error".into();
        assert_eq!(app_err.message, "a str slice error");
    }

    #[test]
    fn serialize_works() {
        let err = AppError {
            message: "serialize me".into(),
        };
        let json = serde_json::to_string(&err).expect("serialization should succeed");
        assert_eq!(json, r#"{"message":"serialize me"}"#);
    }
}
