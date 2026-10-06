use crate::error::CdpError;

/// One CDP command/response exchange.
///
/// A live websocket and a replay script both implement this. `params_json`
/// is a JSON object. The returned string is the CDP `result` value, not the
/// envelope. Implementations must not invent a click the caller did not ask for.
pub trait CdpTransport {
    fn call(&mut self, method: &str, params_json: &str) -> Result<String, CdpError>;
}

/// A boxed transport is a transport, so a server can hold a live websocket or
/// a replay behind one type.
impl<T: CdpTransport + ?Sized> CdpTransport for Box<T> {
    fn call(&mut self, method: &str, params_json: &str) -> Result<String, CdpError> {
        (**self).call(method, params_json)
    }
}
