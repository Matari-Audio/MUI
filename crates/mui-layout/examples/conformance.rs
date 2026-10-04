//! Run independent numerical layout fixtures with warm-cache equivalence.
#[path = "../tests/conformance.rs"]
mod conformance;
fn main() {
    conformance::check();
    println!("layout conformance fixtures passed");
}
