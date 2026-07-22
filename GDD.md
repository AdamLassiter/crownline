# CROWNLINES

## Game Design Document

## 1. High Concept

**Crownlines** is a deterministic turn-based strategy game combining the positional clarity of chess with the territorial development of a compact 4X game.

Each player begins with a traditional chess army inside a fortified royal domain. Beyond the walls lies a larger square-grid world containing rival kingdoms, neutral settlements, promotion sites, rivers, mountain passes and ancient roads.

Pieces move and capture according to familiar chess rules, but their positions also determine how the kingdom expands and functions:

* Pawns claim settlements and establish frontiers.
* Rooks maintain infrastructure and defend corridors.
* Bishops govern distant holdings along diagonals.
* Knights explore, raid and bypass defensive lines.
* Queens provide flexible strategic support.
* Kings anchor the kingdom and remain the ultimate target.

Every turn consists primarily of moving one piece. A move may simultaneously threaten an enemy, defend the King, claim territory, support a settlement or prepare a future promotion.

The design goal is to produce the strategic breadth of a 4X game without losing the simplicity, determinism and visual readability of chess.

---

## 2. Design Pillars

### 2.1 Chess First

The game should remain recognisably chess-like.

* Pieces use standard chess movement.
* Captures are immediate and deterministic.
* There are no health values, attack rolls or damage calculations.
* Check, pins, forks, discovered attacks and sacrifices remain central.
* Most turns contain one primary decision: which piece to move.

New mechanics should increase the meaning of position rather than replace it.

### 2.2 Position Is Economy

Economic strength comes from controlling useful squares and maintaining pieces in useful formations.

Players do not collect large quantities of wood, food or gold. Instead, they gain pieces and strategic opportunities by:

* claiming settlements;
* governing them with major pieces;
* protecting promotion routes;
* maintaining open lines;
* denying those same opportunities to opponents.

A settlement is valuable because of where it is and which pieces must be committed to it.

### 2.3 Few Rules, Many Consequences

Each mechanic should create several kinds of decision.

A single Rook move might:

* defend a threatened Pawn;
* govern a settlement;
* open a route for the Queen;
* abandon another settlement;
* expose the King to a Bishop;
* prepare a future capture.

Depth should arise from interactions between simple systems rather than from numerous unit abilities.

### 2.4 Perfect Information

The core game contains:

* no hidden units;
* no random combat;
* no uncertain movement;
* no concealed technology;
* no procedural event outcomes during play.

Players should be able to understand why they lost and identify alternative decisions.

---

## 3. Game Structure

### 3.1 Players

The initial design supports two players.

The rules may later support three or four kingdoms, but the core game should first be balanced around direct competition. Two-player play best preserves chess concepts such as tempo, initiative and calculated exchanges.

### 3.2 Match Length

Target match length:

* introductory map: 30-45 minutes;
* standard match: 60-90 minutes;
* large campaign map: approximately two hours.

The game should reach meaningful conflict quickly. Players should not spend a long opening phase gathering resources without interacting.

### 3.3 Victory

A player wins by checkmating the opposing King.

There is no separate economic, cultural or technological victory in the core rules.

Expansion matters because it enables:

* additional Pawns;
* Pawn promotion;
* control of key approach routes;
* safer staging positions;
* pressure on the enemy King.

The strategic layer creates the conditions for checkmate rather than functioning as a separate scoring game.

---

## 4. The World Board

### 4.1 Grid

The game uses a square grid.

Square geometry is essential because it preserves:

* Rook ranks and files;
* Bishop diagonals;
* Knight movement;
* Pawn structures;
* familiar attack patterns.

A standard map is approximately 20x20 squares. Exact dimensions may vary by scenario.

### 4.2 Starting Kingdoms

Each player begins inside a fortified domain near one edge of the map.

The domain contains:

* one King;
* one Queen;
* two Rooks;
* two Bishops;
* two Knights;
* eight Pawns.

The formation resembles a compressed chess opening position, but players may have more than one route out of the fortification.

The Keep provides early protection but is not intended to remain permanently secure.

### 4.3 World Features

A standard map contains:

* four to six neutral settlements;
* two to four promotion sites;
* limited forests;
* one or two major rivers;
* mountain or wall formations;
* several important crossings and open lines.

Features should be placed deliberately rather than scattered densely. Each should create a recognisable strategic question.

---

## 5. Turn Structure

On a turn, the active player performs one **Command**.

A Command is normally one legal chess move made by one piece.

After moving, the piece may automatically interact with the square it occupies or influences. This is called its **Realm Effect**.

Examples:

* A Pawn ending on a neutral settlement claims it.
* A Rook attacking a friendly settlement governs it.
* A Pawn ending on a promotion site may begin promotion.
* A Rook ending on a fortification square activates its defences.

Realm Effects are contextual and require no separate action unless otherwise stated.

A player may also choose to **Hold** instead of moving. Holding allows ongoing processes such as settlement development or promotion to advance, but sacrifices tempo.

### Player levers

On every turn, the player chooses:

* which region of the board receives attention;
* whether to attack, develop or defend;
* whether to move a powerful piece away from an existing duty;
* whether an immediate gain is worth weakening the wider position;
* whether to spend tempo moving or allow a local process to complete.

---

## 6. Chess Movement and Combat

Pieces use their standard movement and capture rules.

### 6.1 King

* Moves one square in any direction.
* May not enter check.
* May castle under scenario-specific conditions.
* The King’s defeat ends the game.

### 6.2 Queen

* Moves along ranks, files and diagonals.
* Provides the most flexible military and governing support.
* Is difficult to replace and dangerous to commit far from the King.

### 6.3 Rook

* Moves along ranks and files.
* Excels at defending corridors, operating on roads and governing settlements.
* May activate certain fortifications.

### 6.4 Bishop

* Moves diagonally.
* Governs settlements and sites along long diagonal lines.
* Each Bishop remains bound to one square colour, creating permanent geographic limitations.

### 6.5 Knight

* Moves in the standard L-shaped pattern.
* Leaps over pieces and most terrain barriers.
* Excels at reconnaissance, raids, forks and attacks on infrastructure.
* Has limited ability to support distant holdings.

### 6.6 Pawn

* Moves forward one square.
* May move two squares on its first move where the scenario permits.
* Captures one square diagonally forward.
* Cannot retreat.
* Claims settlements and promotes at designated sites.

Each kingdom has a fixed forward direction. Maps should place kingdoms opposite one another so that their Pawn movement remains intuitive.

### 6.7 Captures

A piece captures by moving onto the occupied enemy square.

There are:

* no hit points;
* no retaliation attacks;
* no defence statistics;
* no combat animations that obscure the outcome.

A defended piece may still be captured. Its defender may then recapture normally.

### Player levers

Combat presents familiar chess decisions:

* exchange equal pieces to improve the wider position;
* sacrifice material to open a route;
* threaten several targets at once;
* pin a governing piece to the King;
* raid an undefended settlement instead of capturing a major piece;
* force an opponent to choose between economic position and royal safety.

---

## 7. Settlements and Expansion

Settlements are fixed neutral locations on the board.

They represent towns, ports, monasteries, mining enclaves or other centres of population. Their mechanical function is deliberately consistent even when their visual identity differs.

### 7.1 Claiming a Settlement

A neutral settlement is claimed when a Pawn ends its move on the settlement square.

The Pawn remains on the square and becomes the settlement’s founder.

Claiming does not immediately produce a new unit.

### 7.2 Governing a Settlement

A claimed settlement is **governed** while it is attacked by one of the owner’s major pieces:

* King;
* Queen;
* Rook;
* Bishop.

Knights and Pawns cannot govern settlements unless modified by a scenario or faction rule.

A governing piece does not need to occupy the settlement. It supports it through its normal movement line.

Intervening pieces and blocking terrain interrupt governance exactly as they interrupt movement.

### 7.3 Settlement Development

A claimed settlement gains one development step at the start of its owner’s turn if:

* its founding Pawn remains present;
* it is governed;
* it is not occupied by an enemy;
* it was governed continuously since the previous turn.

After reaching the required number of development steps, usually three, the settlement becomes **established**.

An established settlement may produce one new Pawn after a further development cycle.

The new Pawn appears on an adjacent legal square chosen by the owner.

Each settlement can normally support only one additional Pawn at a time. This prevents exponential army growth.

### 7.4 Losing a Settlement

An enemy may disrupt a settlement by:

* capturing its founding Pawn;
* occupying the settlement;
* capturing or forcing away its governing piece;
* blocking the line between governor and settlement.

An ungoverned settlement does not lose ownership immediately. Its development pauses.

If an enemy Pawn occupies the settlement, control transfers after that Pawn survives until its owner’s next turn.

### Player levers

Settlements create several choices:

* claim a nearby safe settlement or compete for a more valuable central one;
* commit a Rook to governance or keep it available for combat;
* govern several aligned settlements with one exposed piece;
* block an enemy governing line without capturing the governor;
* attack the settlement founder or the supporting piece;
* allow development to complete or move the Pawn onward toward promotion.

### Example

A player claims a settlement with a central Pawn. A Bishop governs it from six squares away.

The opponent can respond by:

* capturing the Pawn;
* placing a piece in the Bishop’s diagonal;
* threatening the Bishop;
* ignoring the settlement and attacking the King;
* claiming another settlement more quickly.

No separate economic action is required. The conflict is resolved through ordinary piece movement.

---

## 8. Pawn Growth and Promotion

Pawns are the primary source of expansion and long-term military growth.

### 8.1 Commitment

Because Pawns cannot retreat, every advance represents a lasting strategic commitment.

A Pawn may be used to:

* protect the King;
* claim a settlement;
* contest a crossing;
* form a defensive chain;
* advance toward promotion;
* sacrifice itself to open a line.

It cannot perform all of these roles simultaneously.

### 8.2 Promotion Sites

Pawns do not promote merely by reaching the opposite map edge.

Instead, promotion occurs at fixed sites such as:

* royal courts;
* ancient academies;
* sacred shrines;
* frontier citadels.

A Pawn that ends its move on a promotion site becomes a **candidate**.

If it remains on the site until the start of its owner’s next turn, it promotes into:

* Queen;
* Rook;
* Bishop;
* Knight.

The original Pawn is removed and replaced immediately.

Promotion sites are visible to all players and become natural strategic objectives.

### 8.3 Interrupted Promotion

Promotion fails if the Pawn is:

* captured;
* forced off the site;
* placed in a position where resolving the promotion would leave its King in check.

### Player levers

Promotion presents choices between:

* developing the economy with a Pawn or sending it onward;
* promoting quickly into a Knight or Rook;
* attempting a high-value Queen promotion;
* defending a promotion site or attacking elsewhere;
* using the threat of promotion to force an enemy response;
* sacrificing a nearly promoted Pawn to gain a decisive attack.

---

## 9. Piece Roles in the Kingdom

Pieces have no additional activated abilities in the core game. Their strategic identities emerge from movement, governance and terrain interaction.

### 9.1 Rooks: Infrastructure and Stability

Rooks are especially effective at supporting settlements because ranks and files commonly align with roads, bridges and defensive corridors.

A single Rook may govern multiple settlements if it has an uninterrupted line to each.

This is powerful but fragile. One blocking piece may disrupt several holdings simultaneously.

**Player choices:**

* centralise several settlements under one efficient Rook;
* distribute governance across multiple safer pieces;
* place the Rook behind a Pawn frontier;
* move it into battle and pause settlement growth;
* create a long open file at the cost of exposing the Rook.

### 9.2 Bishops: Long-Range Governance

Bishops can support distant holdings without occupying central files.

Their square-colour restriction means each Bishop naturally serves a different portion of the map.

Losing one Bishop may make certain settlements difficult to govern.

**Player choices:**

* claim settlements that align with an existing Bishop;
* alter Pawn movement to open a blocked diagonal;
* exchange a Bishop for an enemy Rook at the cost of governance;
* place settlements on opposite colours to reduce dependence;
* exploit an opponent’s missing light- or dark-square Bishop.

### 9.3 Knights: Disruption

Knights do not efficiently govern territory, but they can bypass the structures that other pieces depend upon.

A Knight may:

* attack a settlement founder behind a defensive line;
* fork a King and governing piece;
* occupy an important blocking square;
* cross rivers without using bridges;
* threaten promotion sites from unusual angles.

**Player choices:**

* use a Knight as a tactical attacker;
* station it defensively near several settlements;
* raid enemy infrastructure rather than pursue material;
* sacrifice it to interrupt a decisive promotion;
* keep it near the King to deter forks and infiltrations.

### 9.4 Queen: Flexibility at a Cost

The Queen can govern along either orthogonal or diagonal lines and rapidly change theatres.

However, using the Queen to support the economy may limit offensive pressure. Sending it into hostile territory may leave several settlements dormant.

**Player choices:**

* use the Queen as an efficient governor;
* centralise it for maximum flexibility;
* commit it to an attack on the enemy King;
* exchange it to remove several enemy governing pieces;
* hold it in reserve while less valuable pieces take risks.

### 9.5 King: Safety and Activity

The King is the victory target but may become an active governing piece later in the game.

A centralised King can support nearby settlements and pieces but is exposed to more lines of attack.

**Player choices:**

* remain protected inside the Keep;
* castle toward developing territory;
* activate the King in a simplified endgame;
* abandon an outer settlement to preserve safety;
* use the King to govern while freeing a Rook or Bishop.

---

## 10. Terrain

Terrain changes board geometry rather than modifying combat statistics.

There are no defence bonuses, accuracy penalties or movement costs.

### 10.1 Forest

A forest square may be entered normally.

However, sliding pieces cannot move or attack through a forest square. A Rook, Bishop or Queen may enter the first forest square in its path but cannot continue beyond it in the same move.

Knights leap normally.

**Choices created:**

* hide important pieces from long attack lines;
* use a forest to block governance;
* occupy the forest to control its exit squares;
* favour Knights in densely wooded regions;
* clear an important line by moving out of the forest.

### 10.2 Mountain

Mountain squares are impassable.

Knights may leap across mountains but may not land on them.

Mountains form permanent walls, creating:

* narrow passes;
* protected flanks;
* constrained diagonals;
* valuable chokepoints.

**Choices created:**

* defend a narrow pass with few pieces;
* send a Knight across the barrier;
* compete for the only open diagonal;
* accept a longer route in exchange for safety.

### 10.3 River

Rivers run along boundaries between squares.

Pieces may cross only at marked bridges or fords.

Knights may leap across a river if their destination is legal.

A bridge does not provide a defence bonus. Its value comes from limiting movement.

**Choices created:**

* contest a bridge directly;
* bypass it with Knights;
* pin a piece responsible for defending the crossing;
* use the bridge as a predictable promotion route;
* surrender one crossing to concentrate on another.

### 10.4 Road

Roads do not increase movement distance.

Instead, roads are placed to create naturally open ranks and files between significant locations. Forests and ruins rarely block them.

Roads are primarily a map-design tool rather than a separate rule system.

### 10.5 Fortification

Fortification squares include Keep towers, gates and frontier bastions.

A Rook occupying a fortification square may project movement and attacks through one adjacent friendly wall segment.

Other pieces treat walls as impassable.

Fortifications therefore improve geometry rather than durability.

**Choices created:**

* station a Rook defensively or release it into the field;
* attack a gate instead of a protected wall;
* seize an enemy tower to gain a new line;
* force the Rook away through a threat elsewhere.

---

## 11. The Royal Keep

The Keep is the starting fortification and political centre of each kingdom.

It contains:

* a protected deployment area;
* two or more gates;
* Rook-compatible towers;
* several safe Pawn starting routes.

The Keep should resist immediate attack but should not make permanent defence optimal.

### 11.1 Castling

Castling follows familiar chess principles but uses designated King and Rook positions.

It is legal only when:

* neither participating piece has moved;
* the path is clear;
* the King is not in check;
* the King does not cross or enter an attacked square.

Castling may relocate the King toward one side of the wider world, making it a strategic commitment to a frontier.

### Player levers

The Keep presents opening choices:

* which gate to open first;
* which side to castle toward;
* whether to release a Rook from a tower;
* whether to retain a defensive Pawn structure;
* when to transition from protected development into open conflict.

---

## 12. Check and Checkmate

Check operates as in chess.

A player may not make a move that leaves their own King under attack.

When placed in check, the player must:

* move the King;
* capture the attacking piece;
* block the attack where possible.

A player who cannot legally escape check loses.

### Strategic implications

Because the board is larger than a chessboard:

* distant attacks require preparation;
* open lines may cross settlements and terrain;
* an economic move may unexpectedly expose the King;
* governing pieces can be pinned and therefore unable to move;
* a player may attack the King to interrupt settlement development or promotion.

Check is therefore both a potential route to victory and a method of stealing tempo.

### Player levers

A player may use check to:

* force an opponent away from a promotion site;
* prevent a settlement from completing development;
* move a governing piece indirectly;
* gain time to reinforce a threatened frontier;
* trade an attacking piece for a positional advantage.

---

## 13. Strategic Tempo

There is no stored action-point resource in the core game.

Tempo is represented directly by turns.

Every move spent on one task is a move not spent elsewhere.

Examples:

* moving a Bishop to govern a settlement may delay an attack;
* defending a Pawn may allow the enemy to claim a bridge;
* checking the enemy King may interrupt their promotion;
* leaving a piece stationary may complete development but concede initiative.

This makes time the game’s primary abstract resource.

### Player levers

Players manage tempo by deciding:

* which threat requires an immediate response;
* when a process is valuable enough to wait for;
* when to abandon sunk development;
* whether to create several simultaneous threats;
* when to trade material for time.

---

## 14. Opening, Midgame and Endgame

### 14.1 Opening

The opening focuses on:

* leaving the Keep;
* selecting settlement routes;
* opening lines for Bishops and Rooks;
* determining King safety;
* contesting bridges and central squares.

Players should encounter one another within the first several turns.

### 14.2 Midgame

The midgame focuses on:

* settlement development;
* raids on governing pieces;
* promotion threats;
* tactical exchanges;
* maintaining several fronts;
* preparing attacks on the enemy Keep.

This phase contains the greatest tension between kingdom management and direct combat.

### 14.3 Endgame

The endgame begins when several major pieces have been exchanged or one King becomes exposed.

Settlements may continue producing Pawns, but fewer governing pieces make them harder to maintain.

Kings become more active, and promoted pieces may decide the match.

The game should naturally simplify as material is removed, preserving the clarity of a chess endgame rather than escalating into an enormous late-game army.

---

## 15. Example Decisions

### Example 1: The Overworked Rook

A Rook currently governs two settlements on the same file. An enemy Bishop attacks one of the player’s Knights.

The Rook could capture the Bishop, but moving it would pause both settlements.

The player may:

* preserve the Knight and delay economic growth;
* allow the Knight to fall and complete two settlements;
* move another piece to block the Bishop;
* create a check elsewhere and force the opponent to respond.

The decision involves material, tempo and development without introducing additional rules.

### Example 2: The Passed Frontier Pawn

A Pawn has claimed a settlement and nearly completed its development. It also has a clear route toward a promotion site.

The player may:

* leave it in place to produce another Pawn;
* abandon the settlement and advance;
* wait until a replacement Pawn arrives;
* use the promotion threat to draw enemy pieces away;
* sacrifice the Pawn to open a Rook line.

The Pawn functions as population, territory and military potential.

### Example 3: The Knight Raid

An enemy Rook governs three aligned settlements.

A Knight can jump into a square attacking both the Rook and one settlement founder.

The opponent must decide whether to:

* save the Rook;
* save the Pawn;
* counterattack the Knight;
* check the raiding player’s King;
* abandon part of the settlement network.

The Knight has disrupted an empire without using a special sabotage ability.

### Example 4: The Bishop Exchange

A Bishop governs a remote settlement and protects a promotion site. It can capture an enemy Rook.

The exchange is materially favourable, but moving the Bishop would:

* pause settlement development;
* leave the promotion site undefended;
* open a diagonal toward the King.

The player must judge the value of the capture within the whole position.

---

## 16. Readability and Interface

The board should communicate strategic information without requiring menus.

### 16.1 Visible Information

When a piece is selected, the interface displays:

* legal moves;
* attacked squares;
* governed settlements;
* lines that would open or close after movement;
* whether the move exposes the King;
* development processes that would pause.

### 16.2 Settlement Presentation

Each settlement displays:

* current owner;
* founding Pawn;
* governing piece;
* current development stage;
* whether its governing line is blocked.

### 16.3 Threat Preview

Before confirming a move, the player may preview:

* newly threatened pieces;
* lost defensive coverage;
* interrupted settlements;
* activated promotion threats.

The interface should help players read the rules, not recommend the best move.

### 16.4 Visual Language

Pieces should remain immediately identifiable by silhouette.

Kingdom visuals may vary through:

* architecture;
* colour;
* banners;
* piece ornamentation;
* settlement appearance.

Mechanical differences should initially be minimal. Cosmetic identity should not reduce board readability.

---

## 17. Asymmetry and Factions

The initial game should use symmetrical kingdoms.

Asymmetric factions may be introduced later through one small rule modification each.

Examples:

* Knights may govern settlements they occupy.
* Castling may occur with either Rook regardless of distance.
* A Bishop may change square colour once per game.
* Settlements require fewer turns to establish but are lost more quickly.
* Pawns may choose between two forward directions when first deployed.

Faction rules should:

* preserve standard movement;
* be visible from the start;
* alter strategic priorities;
* avoid creating decks of activated abilities.

No faction should require a separate technology tree.

---

## 18. Map Design Principles

Maps provide much of the game’s variety.

A strong map creates competing incentives rather than obvious optimal routes.

### Settlement placement

Settlements should differ through position rather than numeric yield.

A settlement may be valuable because it is:

* close to the Keep;
* aligned with a Bishop;
* located beyond a bridge;
* near a promotion site;
* positioned on an open Rook file;
* useful as a staging point for attack.

### Promotion site placement

Promotion sites should:

* be exposed enough to contest;
* require commitment to reach;
* allow several approach routes;
* avoid being permanently dominated by one starting position.

### Terrain density

Terrain should remain sparse.

Each feature should visibly change:

* movement;
* attack lines;
* access;
* governance.

Decorative terrain must not resemble mechanically active terrain.

---

## 19. Scope Boundaries

The core game does not include:

* unit health;
* combat randomness;
* resource inventories;
* worker units;
* city construction menus;
* technology trees;
* equipment;
* unit experience levels;
* hidden information;
* tactical combat sub-screens;
* diplomacy in two-player competitive play.

These systems may be familiar from 4X games, but they would weaken the game’s central identity.

The intended complexity comes from the relationship between pieces, settlements, objectives and board geometry.

---

## 20. Initial Prototype

The first playable prototype should contain:

### Board

* 20x20 square grid;
* two opposing Keeps;
* four neutral settlements;
* two promotion sites;
* one river with two bridges;
* two small forests;
* one mountain barrier with a pass.

### Armies

Each player receives:

* one King;
* one Queen;
* two Rooks;
* two Bishops;
* two Knights;
* eight Pawns.

### Rules

* Standard chess movement and captures.
* One move per turn.
* Standard check and checkmate.
* Pawns claim settlements.
* Kings, Queens, Rooks and Bishops govern settlements through attack lines.
* Governed settlements develop over three turns.
* Established settlements produce one additional Pawn.
* Pawns promote after surviving one turn on a promotion site.
* Terrain modifies movement lines but never combat strength.

### Prototype questions

Testing should determine:

* whether settlement governance is immediately understandable;
* whether settlement growth is too fast or too slow;
* whether economic play produces meaningful conflict;
* whether promotion sites create sufficient pressure;
* whether the large board causes excessive downtime;
* whether Rooks and Bishops become overloaded with non-combat duties;
* whether checkmate remains achievable within the target match length.

---

## 21. Adjustable Design Levers

The following values can be changed during testing without altering the core design.

### Map levers

* board dimensions;
* distance between Keeps;
* number of settlements;
* number of promotion sites;
* number and width of chokepoints;
* openness of central lines.

### Economy levers

* turns required to establish a settlement;
* turns required to produce a Pawn;
* number of Pawns a settlement may produce;
* whether development resets or merely pauses when governance is lost;
* whether one piece may govern multiple settlements.

### Promotion levers

* time required to promote;
* number of promotion sites;
* pieces available through promotion;
* whether promotion sites may be controlled or blocked;
* whether a settlement can also function as a promotion site.

### Combat levers

* starting fortification strength;
* castling rules;
* Pawn first-move distance;
* use of en passant;
* whether captured pieces can ever return;
* restrictions on newly produced Pawns.

### Terrain levers

* whether forests block movement completely or merely block lines;
* whether Knights may cross rivers;
* whether Rooks gain special interaction with fortifications;
* number of permanent walls and passes.

These levers should be tuned carefully. The preferred solution to a balance problem is usually a map or timing adjustment, not the addition of a new exception.

---

## 22. Core Design Test

Every major mechanic should pass the following question:

> Does this mechanic make an ordinary chess move carry additional strategic meaning?

Strong examples include:

* moving a Rook changes both an attack and a settlement network;
* advancing a Pawn changes both the frontier and future promotion potential;
* blocking a Bishop interrupts both a threat and the governance of a town;
* checking the King steals the turn needed to complete an expansion.

A mechanic that requires unrelated menus, numerical optimisation or separate tactical resolution should be removed or redesigned.

The intended result is not a 4X game decorated with chess pieces. It is an expanded form of chess in which the board has become a kingdom.
