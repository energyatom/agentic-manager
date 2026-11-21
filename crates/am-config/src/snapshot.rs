//! The immutable `ConfigSnapshot` (AD-16): every entity by id, keyed by a
//! sha256 over the sorted (path, bytes) pairs of the files it was read from.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use crate::entities::*;

/// One loaded project. Built only when no file had an error; never changed
/// after it is built (a reload builds a new one).
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigSnapshot {
    pub(crate) hash: String,
    pub(crate) company: Company,
    pub(crate) accounts: Vec<Account>,
    pub(crate) prices: Prices,
    pub(crate) capabilities: Capabilities,
    pub(crate) tools: ToolsConfig,
    pub(crate) seats: BTreeMap<String, Seat>,
    pub(crate) workflows: BTreeMap<String, Workflow>,
    pub(crate) tables: BTreeMap<String, Table>,
    pub(crate) schemas: BTreeMap<String, serde_json::Value>,
    pub(crate) dashboards: BTreeMap<String, Dashboard>,
    pub(crate) sources: BTreeMap<String, Source>,
    pub(crate) skills: BTreeMap<String, Skill>,
}

impl ConfigSnapshot {
    /// Lowercase hex sha256 of the project's config files; the same files
    /// give the same hash wherever the project folder is.
    pub fn hash(&self) -> &str {
        &self.hash
    }
    pub fn company(&self) -> &Company {
        &self.company
    }
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }
    pub fn prices(&self) -> &Prices {
        &self.prices
    }
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }
    pub fn tools(&self) -> &ToolsConfig {
        &self.tools
    }
    pub fn seats(&self) -> &BTreeMap<String, Seat> {
        &self.seats
    }
    pub fn workflows(&self) -> &BTreeMap<String, Workflow> {
        &self.workflows
    }
    pub fn tables(&self) -> &BTreeMap<String, Table> {
        &self.tables
    }
    /// Step output schemas (`schemas/<id>.json`) as JSON values.
    pub fn schemas(&self) -> &BTreeMap<String, serde_json::Value> {
        &self.schemas
    }
    pub fn dashboards(&self) -> &BTreeMap<String, Dashboard> {
        &self.dashboards
    }
    pub fn sources(&self) -> &BTreeMap<String, Source> {
        &self.sources
    }
    /// Skill folders (`skills/<id>/`) by id. Instructions only (AD-38).
    pub fn skills(&self) -> &BTreeMap<String, Skill> {
        &self.skills
    }
    /// The host patterns `seat` may reach (AD-12, AD-48): exactly the
    /// seat's own `allowed_hosts`, never an Account's endpoint; none for an
    /// unknown seat.
    pub fn allowed_hosts(&self, seat: &str) -> &[am_domain::HostPattern] {
        self.seats
            .get(seat)
            .map_or(&[], |s| s.allowed_hosts.as_slice())
    }
}

/// sha256 over (path, bytes) pairs sorted by path. Each pair is framed by
/// lengths so that no two different file sets give the same input.
pub(crate) fn content_hash(files: &BTreeMap<String, Vec<u8>>) -> String {
    let mut h = Sha256::new();
    for (path, bytes) in files {
        h.update((path.len() as u64).to_le_bytes());
        h.update(path.as_bytes());
        h.update((bytes.len() as u64).to_le_bytes());
        h.update(bytes);
    }
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(pairs: &[(&str, &str)]) -> BTreeMap<String, Vec<u8>> {
        pairs
            .iter()
            .map(|(p, b)| (p.to_string(), b.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn framing_separates_path_from_bytes() {
        let a = content_hash(&files(&[("ab", "c")]));
        let b = content_hash(&files(&[("a", "bc")]));
        assert_ne!(a, b);
        assert_eq!(a, content_hash(&files(&[("ab", "c")])));
        assert_eq!(a.len(), 64);
    }
}
