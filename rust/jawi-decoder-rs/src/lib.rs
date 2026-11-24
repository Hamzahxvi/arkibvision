//! Python bindings for `src/utils/jawi_decoder.py`: generated from the signatures in `port.rs` by rust-anything.
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

fn normalize_rough_rumi_word_inner(token: String) -> Result<String, String> {
    let out = port::normalize_rough_rumi_word(token)?;
    Ok(out)
}

/// `normalize_rough_rumi_word`.
#[pyfunction]
#[pyo3(name = "normalize_rough_rumi_word", signature = (token))]
fn normalize_rough_rumi_word(token: String) -> PyResult<String> {
    normalize_rough_rumi_word_inner(token).map_err(error)
}

fn decode_jawi_or_rough_rumi_line_inner(line: String) -> Result<serde_json::Value, String> {
    let out = port::decode_jawi_or_rough_rumi_line(line)?;
    Ok(out)
}

/// `decode_jawi_or_rough_rumi_line`.
#[pyfunction]
#[pyo3(name = "decode_jawi_or_rough_rumi_line", signature = (line))]
fn decode_jawi_or_rough_rumi_line(py: Python<'_>, line: String) -> PyResult<Py<PyAny>> {
    decode_jawi_or_rough_rumi_line_inner(line)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

fn decode_jawi_text_inner(text: String) -> Result<serde_json::Value, String> {
    let out = port::decode_jawi_text(text)?;
    Ok(out)
}

/// `decode_jawi_text`.
#[pyfunction]
#[pyo3(name = "decode_jawi_text", signature = (text))]
fn decode_jawi_text(py: Python<'_>, text: String) -> PyResult<Py<PyAny>> {
    decode_jawi_text_inner(text)
        .map_err(error)
        .and_then(|v| crate::pyconv::ToPy::to_py(&v, py))
}

#[pymodule]
#[pyo3(name = "_native")]
fn native_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(normalize_rough_rumi_word, m)?)?;
    m.add_function(wrap_pyfunction!(decode_jawi_or_rough_rumi_line, m)?)?;
    m.add_function(wrap_pyfunction!(decode_jawi_text, m)?)?;
    Ok(())
}
