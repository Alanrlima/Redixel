# Unites War

A playable side-view strategy prototype inspired by the unit production and
fortress combat loop of classic browser war games.

The battlefield uses projected 2.5D geometry built from the engine's triangle
API. Units, castles, controls, projectiles, shadows, and the ground are rendered
as low-poly extruded shapes without requiring a native 3D pipeline.

## Controls

- `Enter` — start the battle from the main menu
- `1` — recruit a runner (30 coins)
- `2` — recruit a guard (50 coins)
- `3` — recruit an archer (65 coins)
- `4` — recruit a brute (100 coins)
- `Q` — cast free lightning at the mouse cursor (15-second cooldown)
- `U` — upgrade the clan (cost increases each level)
- `H` — open or close the skills panel
- `R` — restart the battle
- `Esc` — exit

The initial menu can also be controlled with the mouse through the `JOGAR` and
`SAIR` buttons. Its labels use a built-in 5x7 bitmap font rendered entirely
with Redixel rectangles.

Coins are generated passively during battle and awarded whenever an opposing
unit is defeated. Stronger units grant larger bounties. The four coloured cards
at the bottom can be clicked to recruit units, while the golden fifth card buys
a base upgrade. Clicking the battlefield casts lightning when it is ready and
enough coins are available.

During battle, the `HABILIDADES` button opens a paused shop where coins can buy
one passive for each unit type: Momentum, Bulwark, Piercing Shot, and Rage.
Purchases affect current and future player units for the rest of that battle.
Close the panel with `H`, `Esc`, or the `FECHAR` button.

## Run

```sh
cargo run --release --bin unites_war
```
