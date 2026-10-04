//! Untrusted JSON-RPC lines must not panic. 16 cases. Not a proof.

use hyper_use_mcp::handle_line;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]
    #[test]
    fn random_lines_do_not_panic(raw in "\\PC{0,160}") {
        let _ = handle_line(&raw);
    }
}
