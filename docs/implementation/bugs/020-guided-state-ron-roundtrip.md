# Bug 020: Guided canonical starts fail RON round-trip

## Status

- [x] Done

## Resolution

- Extended the bounded `PieceId` deserializer to accept the single-value newtype sequence emitted by RON as well as the existing numeric JSON value and numeric JSON object-key forms.
- Added direct regression coverage for RON newtype round-trip, malformed RON sequences, and unchanged numeric JSON decoding. Task 14.01.02 separately exercises a complete guided scenario/progress round-trip.
- Malformed empty or multi-value newtype sequences remain rejected.

## Linked task and introducing commit

- [Task 14.01.01](../14-guided-scenarios/14.01-framework/14.01.01-guided-schema.md), commit `fe1b386`, first embedded canonical `MatchState` values in authored RON scenario data and exposed the format mismatch.

## Reproduction

1. Add a validated `guided` block containing a canonical start state to a scenario.
2. Serialize the scenario with RON.
3. Deserialize the serialized RON as `ScenarioDefinition`.

## Expected behavior

The scenario and its stable piece identities round-trip so guided progress can retain the exact authored scenario used by a resume snapshot.

## Actual behavior

RON emits `PieceId` newtypes as single-value sequences, which the custom deserializer rejected with `Expected a numeric piece ID or numeric JSON object key but found a sequence instead`.

## Impact

Guided starts validate in memory but cannot be reconstructed from serialized RON, preventing safe guided save/resume and future authored guided scenario files.

## Dependencies

- 14.01.01.

## Acceptance criteria

- Guided scenarios containing canonical start states round-trip through RON.
- JSON save envelopes and numeric JSON object keys remain compatible.
- Empty, oversized, and multi-value piece IDs remain rejected.
