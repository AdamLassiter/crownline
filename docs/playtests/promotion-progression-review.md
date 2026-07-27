# Promotion progression balance review

## Decision

Retain the initial Bishop/Rook/Queen thresholds at 2/4/8 for Introductory,
Standard, and Large. The deterministic evidence shows a clean strategic ladder:
a direct promotion rush recruits only a Knight, while Queen requires materially
broader and more defensible realm development.

## Reproducible evidence

The archived [`automated-promotion-progression.json`](automated-promotion-progression.json)
is pinned to each scenario's canonical hash. Its generator and integration test
construct score 0, 2, 4, and 8 positions while retaining the shipped board,
terrain, edge, settlement, and promotion-site definitions. Governors must form
real unblocked lines under canonical rules. Every reported recruit is then
enumerated and applied through the reducer rather than inferred from UI copy.

| Scenario | Board | Settlements | Promotion sites | Maximum full control | Rush Queen | Score-8 Queen |
| --- | ---: | ---: | ---: | ---: | --- | --- |
| The First Crossing | 16x16 | 4 | 2 | 16 | Locked | Legal |
| Crownlines | 20x20 | 6 | 4 | 24 | Locked | Legal |
| The Three Theatres | 24x24 | 6 | 4 | 24 | Locked | Legal |

All maps have enough distinct settlements and promotion sites to reach every
tier. Their maximum current-control score exceeds 8, so the Queen threshold is
reachable without requiring total map ownership.

## Rush versus realm-control comparison

| Approach | Control breakdown | Score | Available recruits | Interpretation |
| --- | --- | ---: | --- | --- |
| Promotion rush | 0 owned, 0 governed, 0 established | 0 | Knight | Reaching and defending a site still matters, but does not immediately create the strongest material. |
| Claim and govern one settlement | 1 owned, 1 governed, 0 established | 2 | Knight, Bishop | A fragile but actively supported claim earns the first positional recruit. |
| Establish one governed settlement | 1 owned, 1 governed, 1 established | 4 | Knight, Bishop, Rook | Rook requires sustained defense through establishment, not elapsed match duration alone. |
| Establish two governed settlements | 2 owned, 2 governed, 2 established | 8 | Knight, Bishop, Rook, Queen | Queen requires two complete holdings or comparably broad current control. |

The 2-point Bishop step rewards engaging with the realm layer before the full
three-cycle establishment delay. The 4-point Rook step gives one successfully
developed settlement a meaningful payoff. The 8-point Queen step prevents the
old immediate-Queen rush while requiring a second defended objective. No shipped
threshold needs tuning from this evidence.

The regression suite also covers exact boundary values, governance gain/loss,
ownership transfer, same-boundary establishment and transfer ordering,
current-control relocking, and a frozen simultaneous-promotion batch. A choice
already queued never changes when live control changes.

## Limits

This review is automated and deliberately does not masquerade as consented human
playtest evidence. It does not establish subjective enjoyment, UI comprehension,
full-match duration, geographic bias, or whether 2/4/8 is optimal after strong
human counterplay. Those broader questions remain in Task 10.03.01 and must use
the name-free `F8` side-swapped capture protocol before scenario tuning based on
human experience.
