# Crownlines implementation backlog

This directory decomposes the [game design document](../../GDD.md) into an ordered set of epics, stories, and executable tasks. Numeric prefixes are the proposed implementation sequence. Work inside an epic may proceed in parallel only after its listed dependencies are satisfied.

Development changes follow the [task and bug workflow](00-development-workflow.md). Status in an issue file is authoritative; the ordering index is not itself a completion tracker.

## Definition of done

Every completed item must:

- satisfy its local acceptance criteria;
- preserve the invariants in [cross-cutting concerns](00-cross-cutting-concerns.md);
- include automated tests proportional to its risk;
- pass formatting, linting, and the relevant workspace test suites;
- update player, developer, or operator documentation when behavior changes.

Development should be managed as:

- one commit per task, including updating the task to complete, additional implementation notes, and a commit message linking back to the task
- bugs should be raised when discovered and linked back to the task that introduced them

## Ordered epics

1. [Foundation](01-foundation/01-epic.md)
2. [Core domain and persistence](02-core-domain/02-epic.md)
3. [Chess rules and terrain geometry](03-rules-geometry/03-epic.md)
4. [Realm systems and match flow](04-realm-systems/04-epic.md)
5. [Authored scenarios](05-scenarios/05-epic.md)
6. [Board rendering](06-rendering/06-epic.md)
7. [Interaction and local play](07-local-client/07-epic.md)
8. [Online protocol and authoritative server](08-online-server/08-epic.md)
9. [Online client](09-online-client/09-epic.md)
10. [Quality, balance, and performance](10-quality/10-epic.md)
11. [Release and operations](11-release/11-epic.md)

## Initial release boundaries

- Two symmetric players, using full chess armies.
- Local hot-seat and private-room online play.
- Introductory 16x16, standard 20x20, and large 24x24 maps.
- No AI, factions, public matchmaking, ratings, chat, spectators, campaign, procedural maps, editor, mobile client, or browser client.
