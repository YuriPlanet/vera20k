//! Browser I/O only. The parent serial driver owns protocol decisions and state.

use super::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = globalThis, js_name = veraLobbyOpen)]
    fn open(origin: &str, service: Option<&str>) -> Result<u32, JsValue>;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLobbyRequest)]
    fn fetch(
        id: u32,
        path: &str,
        token: Option<&str>,
        body: Option<&str>,
        limit: u32,
    ) -> js_sys::Promise;
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLobbyCancel)]
    fn cancel(id: u32);
    #[wasm_bindgen(js_namespace = globalThis, js_name = veraLobbyDelay)]
    fn timeout(ms: u32) -> js_sys::Promise;
}

/// Lives with the original LiveService, including its retiring visit. Dropping
/// it cancels only that visit's requests; no callback can enter a later visit.
pub(in crate::app::shell_wol::live_service) struct RequestOwner(u32);

impl Drop for RequestOwner {
    fn drop(&mut self) {
        cancel(self.0);
    }
}

pub(super) async fn delay(ms: u32) {
    let _ = JsFuture::from(timeout(ms)).await;
}

impl Client {
    pub(super) async fn request(
        &self,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, RequestFailure> {
        if self.stopping.load(Ordering::Acquire) {
            return Err(RequestFailure::Status(0));
        }
        let body = body.map(Value::to_string);
        let value = JsFuture::from(fetch(
            self.browser,
            path,
            self.token.as_deref(),
            body.as_deref(),
            MAX_BODY_BYTES as u32,
        ))
        .await
        // Fetch errors can include URLs or server text. Retain only the existing
        // numeric failure consumed by the original retry/cleanup owner.
        .map_err(|_| RequestFailure::Status(0))?;
        if self.stopping.load(Ordering::Acquire) {
            return Err(RequestFailure::Status(0));
        }
        let encoded = value.as_string().ok_or(RequestFailure::Status(0))?;
        let (status, body): (u16, String) =
            serde_json::from_str(&encoded).map_err(|_| RequestFailure::Status(0))?;
        if !(200..300).contains(&status) {
            return Err(RequestFailure::response(status, body.as_bytes()));
        }
        serde_json::from_str(&body).map_err(|_| RequestFailure::Status(0))
    }
}

pub(in crate::app::shell_wol::live_service) fn start(
    input: Receiver<Request>,
    output: Sender<Response>,
    stopping: Arc<AtomicBool>,
    service_url: Option<&str>,
) -> anyhow::Result<RequestOwner> {
    // The lobby shares the gateway's origin; an external launch names its
    // own match API origin (docs/spawn.md).
    let id = open(crate::net::udp::browser_proxy_url(), service_url)
        .map_err(|_| anyhow::anyhow!("could not initialize the browser lobby transport"))?;
    let client = Client {
        browser: id,
        stopping: Arc::clone(&stopping),
        token: None,
        cursor: 0,
        active_match: None,
    };
    let (jobs, translations) = std::sync::mpsc::channel::<(u64, String, String, &'static str)>();
    let mut translator = Client {
        browser: id,
        stopping: Arc::clone(&stopping),
        token: None,
        cursor: 0,
        active_match: None,
    };
    let translated = output.clone();
    wasm_bindgen_futures::spawn_local(async move {
        while !translator.stopping.load(Ordering::Acquire) {
            let (id, token, text, target) = match translations.try_recv() {
                Ok(job) => job,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    delay(50).await;
                    continue;
                }
            };
            translator.token = Some(token);
            let body = json!({"text":text,"target":target});
            let outcome =
                translation_result(translator.request("/v1/translate", Some(&body)).await);
            if translator.stopping.load(Ordering::Acquire)
                || translated
                    .send(Response::Translated { id, outcome })
                    .is_err()
            {
                break;
            }
        }
    });
    // The existing retirement owner sets stopping before waiting for all
    // response senders to close. Cancel an in-flight fetch without waiting for
    // its twelve-second network timeout; this task owns no response sender.
    let watch = Arc::clone(&stopping);
    wasm_bindgen_futures::spawn_local(async move {
        while !watch.load(Ordering::Acquire) {
            delay(50).await;
        }
        cancel(id);
    });
    wasm_bindgen_futures::spawn_local(async move {
        run_loop(client, input, output, Some(jobs)).await;
        stopping.store(true, Ordering::Release);
        cancel(id);
    });
    Ok(RequestOwner(id))
}
