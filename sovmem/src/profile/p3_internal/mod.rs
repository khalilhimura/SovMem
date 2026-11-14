use super::p2_fixture as p2;
pub mod p3;

pub(crate) mod support {
    #[allow(unused_imports)]
    pub use super::p3;
}

mod advisory_erasure;
mod canon;
mod capability;
mod determinism;
mod fixture_concurrency;
mod full_loop;
mod index;
mod ledger;
mod poisoning_matrix;
mod policy;
mod proposal;
mod store;
