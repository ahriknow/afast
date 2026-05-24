#[cfg(feature = "code")]
use crate::{Lang, Service};

/// Dispatches code generation for a single service to the appropriate language backend.
///
/// Returns the generated source code as a string, or [`CodeError`] if the service
/// is not found or the requested language backend is not compiled in.
#[cfg(feature = "code")]
#[allow(unused_variables)]
pub fn generate_code(
    services: &[Service],
    service_name: &str,
    lang: &Lang,
) -> Result<String, CodeError> {
    let svc = services
        .iter()
        .find(|s| s.name == service_name)
        .ok_or_else(|| CodeError::ServiceNotFound(service_name.to_string()))?;

    match lang {
        #[cfg(feature = "ts")]
        Lang::TS(calls) => Ok(super::ts::generate_service_ts(svc, calls, false)),
        #[cfg(not(feature = "ts"))]
        Lang::TS(_) => Err(CodeError::LangNotEnabled("ts".to_string())),
        #[cfg(feature = "js")]
        Lang::JS(calls) => Ok(super::js::generate_service_js(svc, calls, false)),
        #[cfg(not(feature = "js"))]
        Lang::JS(_) => Err(CodeError::LangNotEnabled("js".to_string())),
        #[cfg(feature = "kt")]
        Lang::KT(calls) => Ok(super::kt::generate_service_kt(svc, calls, false)),
        #[cfg(not(feature = "kt"))]
        Lang::KT => Err(CodeError::LangNotEnabled("kt".to_string())),
    }
}

/// Errors from on-demand code generation via the `/code` endpoint.
#[cfg(feature = "code")]
pub enum CodeError {
    /// The requested service name was not found in the registered services.
    ServiceNotFound(String),
    /// The requested language feature is not enabled at compile time.
    #[cfg(not(all(feature = "ts", feature = "js", feature = "kt")))]
    LangNotEnabled(String),
}

#[cfg(feature = "code")]
impl std::fmt::Display for CodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodeError::ServiceNotFound(name) => write!(f, "service '{}' not found", name),
            #[cfg(not(all(feature = "ts", feature = "js", feature = "kt")))]
            CodeError::LangNotEnabled(lang) => {
                write!(f, "language '{}' generation is not enabled", lang)
            }
        }
    }
}
