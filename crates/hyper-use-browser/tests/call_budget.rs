//! Protocol-call budget: `observe()` is the hot path of the whole agent loop —
//! every predict/act/observe cycle calls it, and its CDP round trips are the
//! wall-clock cost driver (the executor re-observes before every input too).
//! These tests pin the current call count so a regression (more calls for the
//! same page) fails CI, and record the number a merged observe must beat.

use hyper_use_browser::script::{Control, PageSpec, ScriptBuilder};
use hyper_use_browser::{BrowserSession, CdpError, CdpTransport, ReplayTransport};

struct CountingTransport {
    inner: ReplayTransport,
    calls: usize,
}

impl CountingTransport {
    fn new(script: ScriptBuilder) -> Self {
        Self {
            inner: ReplayTransport::parse(&script.to_json()).unwrap(),
            calls: 0,
        }
    }
}

impl CdpTransport for CountingTransport {
    fn call(&mut self, method: &str, params_json: &str) -> Result<String, CdpError> {
        self.calls += 1;
        self.inner.call(method, params_json)
    }
}

/// A small but realistic page: nav links, a search field, a submit button —
/// the shape the Flights / Wikipedia demos are made of.
fn small_page() -> PageSpec {
    PageSpec::of(
        &[
            Control::link(10, 100, "Home", (8.0, 8.0, 60.0, 20.0)),
            Control::link(11, 101, "Flights", (80.0, 8.0, 60.0, 20.0)),
            Control::link(12, 102, "Hotels", (152.0, 8.0, 60.0, 20.0)),
            Control::text_field(13, 103, "Search", (280.0, 8.0, 240.0, 28.0)),
            Control::button(14, 104, "Go", (532.0, 8.0, 48.0, 28.0)),
        ],
        "http://127.0.0.1/search",
        "Search",
    )
}

fn dense_page() -> PageSpec {
    // ~50 controls: a dense form/list page.
    let controls: Vec<Control> = (0..50)
        .map(|i| {
            let x = 8.0 + f64::from(i % 5) * 140.0;
            let y = 40.0 + f64::from(i / 5) * 36.0;
            Control::button(
                100 + i64::from(i),
                200 + i64::from(i),
                &format!("Item {i}"),
                (x, y, 128.0, 28.0),
            )
        })
        .collect();
    PageSpec::of(&controls, "http://127.0.0.1/dense", "Dense")
}

#[test]
fn observe_small_page_stays_under_call_budget() {
    let mut session = BrowserSession::new(CountingTransport::new(
        ScriptBuilder::new().observe(&small_page()),
    ));
    session.observe().unwrap();
    let calls = session.transport().calls;
    // Baseline measured 2026-10-06: 25 calls for 5 controls (~4 calls per
    // node + fixed page calls). The invariant is the budget, not the exact
    // count: observe must not grow per-node calls beyond this ceiling. Lower
    // it when observe merges calls.
    assert!(
        calls <= 40,
        "observe issued {calls} CDP calls on a 5-control page (budget 40)"
    );
}

#[test]
fn observe_dense_page_stays_under_call_budget() {
    let mut session = BrowserSession::new(CountingTransport::new(
        ScriptBuilder::new().observe(&dense_page()),
    ));
    session.observe().unwrap();
    let calls = session.transport().calls;
    // Baseline measured 2026-10-06: 205 calls for 50 controls (~4 calls per
    // node: box model, computed style, AX node, hit test). A call-count
    // ceiling here is the regression tripwire for the observe-cost work.
    assert!(
        calls <= 400,
        "observe issued {calls} CDP calls on a 50-control page (budget 400)"
    );
}
