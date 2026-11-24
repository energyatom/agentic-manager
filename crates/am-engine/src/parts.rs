//! Story 3.5 (AD-10, review R3): the one place besides `run.rs` that sorts a
//! journaled step output into a request part, so the files allowed to build
//! an `Instruction` (the spike driver among them) never name the trusted or
//! untrusted types.

use am_domain::{Part, Taint, TrustedInput};

/// A step output as the request holds it: trusted from a clean producer,
/// fenced (same label) from a tainted one, by the producer's committed
/// taint (AD-10 (a)).
pub(crate) fn sort_output(
    label: String,
    text: String,
    producer: Taint,
    trusted: &mut Vec<Part>,
    fenced: &mut Vec<Part>,
) {
    match TrustedInput::from_step_output(label, text, producer) {
        part @ Part::Trusted(_) => trusted.push(part),
        part => fenced.push(part),
    }
}
