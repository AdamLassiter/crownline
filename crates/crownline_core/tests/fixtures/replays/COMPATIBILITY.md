# Golden replay compatibility

These reviewed fixtures pin journal format 2, scenario schema 2, application label `0.1.0-golden`, every intermediate event list, and every canonical state hash.

## Current compatibility note

- Format 1/schema 1 established the initial golden baseline across every shipped scenario and terminal reason plus one compact combined realm path.
- Format 2/schema 2 adds frozen promotion eligibility and explicit 2/4/8 promotion unlock rules. All fixture headers changed; canonical hashes changed only from the combined fixture's promotion-ready revision onward because eligibility became canonical state. Format 1 journals are accepted through the scenario-aware migration reader, which replays actions and regenerates events and hashes; migration fails actionably if an old action is illegal under the progression rule.

## Update policy

Any fixture change must include an explicit note in this section describing which hashes, events, actions, format, or scenario semantics changed and whether old persisted journals remain replay-compatible. Regenerate with `cargo run -p crownline_core --example generate_golden_replays`, review the semantic diff, and commit the generator, fixtures, note, and task update together.

Fixture IDs, idempotency keys, actions, and elapsed clock inputs are deterministic. The files intentionally contain no wall-clock timestamps, random identifiers, player names, credentials, or host-specific data.
