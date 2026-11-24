//! Python bindings for `src/utils/metadata_utils.py`: generated from the signatures in `port.rs` by rust-anything.
//! Regenerate with `rust-anything refresh` after changing them.

#![allow(clippy::too_many_arguments)]

use pyo3::prelude::*;

pub mod js;
pub mod port;
pub mod pyconv;
pub mod pynum;

#[allow(unused_imports)]
use pynum::Num;

fn error(message: String) -> PyErr {
    use pyo3::exceptions as e;
    /// `Err("Kind: text")` for a `Kind` the match below does not know: a built-in exception's name or
    /// `package.module.Class`, called with the text. None when it names no `Exception` subclass or the
    /// class cannot be made from the text alone (its constructor takes more arguments).
    fn named_error(kind: &str, text: &str) -> Option<PyErr> {
        Python::attach(|py| {
            // Each dotted part must be a Python identifier, by Python's own rules (`str.isidentifier`).
            let identifier = |part: &str| {
                pyo3::types::PyString::new(py, part)
                    .call_method0("isidentifier")
                    .and_then(|ok| ok.extract::<bool>())
                    .unwrap_or(false)
            };
            if !kind.split('.').all(identifier) {
                return None;
            }
            let (module, name) = kind.rsplit_once('.').unwrap_or(("builtins", kind));
            let class = py.import(module).ok()?.getattr(name).ok()?;
            let class = class.cast_into::<pyo3::types::PyType>().ok()?;
            if !class
                .is_subclass_of::<pyo3::exceptions::PyException>()
                .ok()?
            {
                return None;
            }
            let value = class.call1((text,)).ok()?;
            if !value.is_instance(&class).ok()? {
                return None;
            }
            Some(PyErr::from_value(value))
        })
    }
    let (kind, text) = message.split_once(": ").unwrap_or(("", message.as_str()));
    let text = text.to_string();
    match kind {
        "TypeError" => e::PyTypeError::new_err(text),
        "KeyError" => e::PyKeyError::new_err(text),
        "IndexError" => e::PyIndexError::new_err(text),
        "OverflowError" => e::PyOverflowError::new_err(text),
        "ZeroDivisionError" => e::PyZeroDivisionError::new_err(text),
        "AttributeError" => e::PyAttributeError::new_err(text),
        "RuntimeError" => e::PyRuntimeError::new_err(text),
        "NotImplementedError" => e::PyNotImplementedError::new_err(text),
        "LookupError" => e::PyLookupError::new_err(text),
        "ArithmeticError" => e::PyArithmeticError::new_err(text),
        "UnicodeError" => e::PyUnicodeError::new_err(text),
        "ValueError" => e::PyValueError::new_err(text),
        _ => named_error(kind, &text).unwrap_or_else(|| e::PyValueError::new_err(message)),
    }
}

fn profile_slug_inner(identity: String) -> Result<String, String> {
    let out = port::profile_slug(identity)?;
    Ok(out)
}

/// `profile_slug`.
#[pyfunction]
#[pyo3(name = "profile_slug", signature = (identity))]
fn profile_slug(identity: String) -> PyResult<String> {
    profile_slug_inner(identity).map_err(error)
}

fn format_identity_name_inner(identity: String) -> Result<String, String> {
    let out = port::format_identity_name(identity)?;
    Ok(out)
}

/// `format_identity_name`.
#[pyfunction]
#[pyo3(name = "format_identity_name", signature = (identity))]
fn format_identity_name(identity: String) -> PyResult<String> {
    format_identity_name_inner(identity).map_err(error)
}

fn format_time_12h_inner(value: String) -> Result<String, String> {
    let out = port::format_time_12h(value)?;
    Ok(out)
}

/// `format_time_12h`.
#[pyfunction]
#[pyo3(name = "format_time_12h", signature = (value))]
fn format_time_12h(value: String) -> PyResult<String> {
    format_time_12h_inner(value).map_err(error)
}

fn default_detail_visibility_inner(key: String) -> Result<String, String> {
    let out = port::default_detail_visibility(key)?;
    Ok(out)
}

/// `default_detail_visibility`.
#[pyfunction]
#[pyo3(name = "default_detail_visibility", signature = (key))]
fn default_detail_visibility(key: String) -> PyResult<String> {
    default_detail_visibility_inner(key).map_err(error)
}

fn parse_proposed_metadata_inner(raw: String) -> Result<serde_json::Value, String> {
    let out = port::parse_proposed_metadata(raw)?;
    Ok(out)
}

/// `parse_proposed_metadata`.
#[pyfunction]
#[pyo3(name = "parse_proposed_metadata", signature = (raw))]
fn parse_proposed_metadata(py: Python<'_>, raw: String) -> PyResult<Py<PyAny>> {
    parse_proposed_metadata_inner(raw)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(profile_slug, m)?)?;
    m.add_function(wrap_pyfunction!(format_identity_name, m)?)?;
    m.add_function(wrap_pyfunction!(format_time_12h, m)?)?;
    m.add_function(wrap_pyfunction!(default_detail_visibility, m)?)?;
    m.add_function(wrap_pyfunction!(parse_proposed_metadata, m)?)?;
    Ok(())
}
