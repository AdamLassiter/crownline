# Golden replay compatibility

These reviewed fixtures pin journal format 1, scenario schema 1, application label `0.1.0-golden`, every intermediate event list, and every canonical state hash.

## Current compatibility note

- Initial golden baseline. It covers every shipped scenario and terminal reason plus one compact combined realm path. No prior golden fixture is superseded.

## Update policy

Any fixture change must include an explicit note in this section describing which hashes, events, actions, format, or scenario semantics changed and whether old persisted journals remain replay-compatible. Regenerate with `cargo run -p crownline_core --example generate_golden_replays`, review the semantic diff, and commit the generator, fixtures, note, and task update together.

Fixture IDs, idempotency keys, actions, and elapsed clock inputs are deterministic. The files intentionally contain no wall-clock timestamps, random identifiers, player names, credentials, or host-specific data.
