# Structured playtesting protocol

This protocol produces the evidence required by Tasks 05.01.02, 05.02.02,
05.03.02, and 10.03.01 without telemetry or identifying participants.

## Before each pair

1. Record the exact application version/build revision and use an unmodified
   shipped scenario. The exported JSON records scenario schema and canonical
   scenario hash automatically.
2. Obtain consent to retain non-identifying gameplay observations. Do not put
   names, contact details, room information, or credentials in the review.
3. Assign neutral roles A and B. In game one, A plays South and B North. In game
   two, swap sides. Use a mirrored opening plan: each role should pursue the
   coordinate rotated through the board centre when its side changes.
4. Keep the same clock configuration, break policy, build, hardware, and
   scenario for both games. Record hardware/build profile separately for the
   24x24 performance pair.

The client holds a name-free report in memory for every local match. Press `F8`
during play, pause, or outcome to explicitly write it under the platform's
`Crownlines/playtests` data directory. Nothing is uploaded. Loading a match
starts a report marked `partial_record`; do not use a partial report as a full
match unless paired with the earlier segment and explained in observations.

## After each game

The JSON already contains version/scenario identity, action/event/state-hash
trace, active/paused/mandatory-choice duration, turn and revision counts, first
capture/check/settlement interaction, claims, produced/promoted Pawns, checks,
geographic move counts, and outcome. Fill the blank `qualitative_review` fields
with concise, consented observations:

- `governance_clarity`: could each role explain every advance, pause, governor,
  and blocker from the board and settlement panel?
- `growth_speed`: when did establishment and production feel too early, useful,
  or irrelevant?
- `economic_conflict`: did settlement ownership provoke interaction rather than
  parallel solitaire development?
- `promotion_pressure`: did promotion sites affect defence and route choice?
- `downtime`: identify long forced marches, mandatory-choice stalls, or turns
  with no meaningful decision.
- `major_piece_overload`: did long-range pieces create unreadable or dominant
  control networks?
- `checkmate_viability`: did the position simplify toward comprehensible mating
  threats, or only toward draw/resignation?
- `observations`: comprehension problems, decisive coordinates, technical
  failures, or explicit tuning hypotheses.

Do not change the automatically captured fields. Store the two reports together
with a pair identifier in their surrounding directory or review index; the
report intentionally contains no participant identity.

## Tuning discipline

Aggregate side-swapped pairs before changing data. Compare South/North outcome,
first-contact turn, geographic move distribution, claims, production,
promotions, checks, and duration. Prefer scenario coordinates and existing
establishment/production/promotion timing values over rule exceptions.

Every change record must state:

1. observed evidence and affected report filenames;
2. a falsifiable hypothesis;
3. the exact scenario-data change and canonical hash;
4. a new side-swapped pair using the changed data;
5. whether the follow-up supported or rejected the hypothesis.

Keep pre-change reports. A rejected hypothesis is evidence, not a reason to
rewrite the earlier record.
