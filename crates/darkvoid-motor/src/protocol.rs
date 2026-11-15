use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct Error {
    pub code: &'static str,
    pub message: &'static str,
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn error(code: &'static str, message: &'static str) -> Error {
    Error { code, message }
}
pub fn invalid() -> Error {
    error("invalid_params", "Parâmetros inválidos.")
}

#[derive(Deserialize)]
pub struct Request {
    pub id: Value,
    pub cmd: String,
    #[serde(default = "empty")]
    pub params: Value,
}
fn empty() -> Value {
    json!({})
}

pub fn parse(text: &str) -> std::result::Result<Request, Value> {
    let value: Value = serde_json::from_str(text).map_err(|_| response(Value::Null, Err(error("invalid_json", "JSON inválido."))))?;
    let id = value.get("id").cloned().unwrap_or_default();
    if !(id.is_u64() || id.is_string()) {
        return Err(response(id, Err(error("invalid_request", "Pedido inválido."))));
    }
    let request: Request = serde_json::from_value(value).map_err(|_| response(id.clone(), Err(error("invalid_request", "Pedido inválido."))))?;
    if !request.params.is_object() {
        return Err(response(id, Err(invalid())));
    }
    Ok(request)
}
pub fn response(id: Value, result: Result<Value>) -> Value {
    match result {
        Ok(result) => json!({"id": id, "ok": true, "result": result}),
        Err(e) => json!({"id": id, "ok": false, "error": {"code": e.code, "message": e.message}}),
    }
}
pub fn field<T: serde::de::DeserializeOwned>(params: Value) -> Result<T> {
    serde_json::from_value(params).map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn responses_keep_id_and_reject_bad_envelopes() {
        let r = parse(r#"{"id":7,"cmd":"hello"}"#).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(response(r.id, Ok(json!({})))["id"], 7);
        assert_eq!(parse("bad").err().unwrap()["error"]["code"], "invalid_json");
        assert!(parse(r#"{"cmd":"hello"}"#).is_err());
        assert!(parse(r#"{"id":1,"cmd":"hello","params":[]}"#).is_err());
    }
}
