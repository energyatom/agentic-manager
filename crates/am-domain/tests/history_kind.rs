//! Story 2.13 (AD-15 *Migrate*, AD-16): `HistoryKind::Removal`, the History
//! entry of the one file removal the config writer makes (a project's old
//! `models/accounts.yaml`, inside a `ConfigMigration`): its `before` is the
//! removed bytes and its `after` empty. It is stored as `removal` and read
//! back from that name; every earlier kind keeps its name. Written before
//! the kind exists.
#![forbid(unsafe_code)]

use am_domain::HistoryKind;

#[test]
fn the_removal_kind_is_stored_as_removal_and_read_back() {
    assert_eq!(HistoryKind::Removal.as_str(), "removal");
    assert_eq!(HistoryKind::parse("removal"), Some(HistoryKind::Removal));
}

#[test]
fn every_kind_round_trips_through_its_stored_name() {
    for kind in [
        HistoryKind::Edit,
        HistoryKind::Undo,
        HistoryKind::Restore,
        HistoryKind::OutsideEdit,
        HistoryKind::Migration,
        HistoryKind::Create,
        HistoryKind::Removal,
    ] {
        assert_eq!(HistoryKind::parse(kind.as_str()), Some(kind), "{kind:?}");
    }
    // The earlier names are unchanged.
    assert_eq!(HistoryKind::Migration.as_str(), "migration");
    assert_eq!(HistoryKind::Create.as_str(), "create");
    assert_eq!(HistoryKind::parse("remove"), None);
    assert_eq!(HistoryKind::parse("Removal"), None);
}
