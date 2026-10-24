# Pixel Horde

A horde defence game where every enemy is a single pixel. Up to millions of zombies, alien bugs or orcs pour through a city's streets like water while you hold an airport with walls, turrets, an economy, rescue squads, troops and missile strikes.

It is a native Windows game written in Rust. The horde is simulated in parallel on every CPU core and drawn on the GPU.

## Play on Windows

1. Open the [latest release](https://github.com/Littlebutta/Horde-defence-game/releases/latest) and download `PixelHorde.exe`.
2. Put it in any folder and double-click it. Windows SmartScreen may warn about an unknown app the first time: click **More info**, then **Run anyway**.

That's all. No Git, browser or installer needed. Every time the game starts it checks this repo's releases; when a newer build exists it downloads it in the background and a **Restart now** button appears on the menu.

Each push to `main` builds a new `PixelHorde.exe` with GitHub Actions and publishes it as release `build-N`. The update check needs the repo to be public.

## Controls

| Input | Action |
| --- | --- |
| 1 to 7, O, L | Wall, gun turret, cannon, scrap extractor, ammo plant, missile silo, extraction pad, outpost, research lab |
| 8 / 9 | Rifle squad / tank, deployed from the HQ to where you click |
| Left click / drag | Place. Drag walls along a street; drag other buildings to lay a row or grid |
| Left click a unit, right click the map | Select troops, then move them (rifles can garrison a building) |
| R / Q / G / E | Rescue squad / airstrike / gunship / ICBM (panel on the right, greyed out until ready) |
| X | Sell the structure under the cursor for 50% |
| Left drag on empty map, middle drag, WASD | Pan |
| Scroll | Zoom |
| T | 2.5D or top-down view |
| K | Dark or light map (dark is the default) |
| H / F | Jump to HQ / fit the whole map |
| Space | Pause |

- Turrets cost more the further they are from the HQ; the extra is shown at the cursor. Outside the base zone they are flown in by helicopter.
- Click a rooftop with a turret selected to mount it there for +60% range. If the building falls, so does the turret.
- Outposts push the base zone outwards. Factories, silos, pads and labs must go inside the zone.
- Rescue squads walk up to civilians one by one and escort them back. Once inside the base, civilians walk to the nearest extraction pad. Each pad flies up to 16 out (+100 score, +20 scrap, +10 ammo each), then needs 30 seconds, so more pads means faster extraction. A crowded base slows your income.
- Every missile silo has its own ICBM cooldown.
- The research lab unlocks upgrades: bigger rescue squads, carpet bombing (drag to aim the bomb run) and a nuclear warhead that leaves radiation nobody of yours can enter or build in.
- Hordes and civilians move along streets, plazas, parks, airport ground and the rubble of destroyed buildings.

## Factions

- **Zombies:** small outbreaks that grow by eating civilians; every bite turns one civilian into two zombies. A zombie that dies gets back up once. When one is killed, the zombies around it speed up and swarm whatever killed it, and stay locked on until it's dead. They wander, attack what they see, and periodically rush the HQ.
- **Alien bugs:** small drops all over the city, straight for the HQ. Some drops are lighter-coloured flyers that cross rooftops and walls. Hives take root now and then: they spawn bugs, stop you building nearby, grow, and infect buildings around them, each one making the hive's waves bigger. Turrets can't hurt a hive; use abilities or troops.
- **Orcs:** portals pour out huge waves that destroy everything they see. Orcs build rage as units around them die, turning redder and running faster. They sometimes grab civilians and drag them back through a portal (troops nearby can free them); every capture brings more orcs out at full rage.

## Building from source

```
cargo build --release
```

The exe is at `target/release/PixelHorde.exe` (or `PixelHorde` on Linux/macOS). Builds without `BUILD_NUMBER` set report themselves as development builds and don't self-update.

For headless testing on Linux: `PH_SHOT=out.png PH_FAC=1 PH_TICKS=2000 xvfb-run ./target/release/PixelHorde` runs a random city for 2000 ticks and saves a screenshot. Add `PH_STRESS=1` to drop a million units on the map, or `PH_SCRIPT=1` to build a base and fire every ability.

## How it works

- **Swarm:** units live in one flat array that rayon splits across all cores each tick. Every unit reads shared grids (open streets, flow fields, last tick's crowd density, queued damage) and writes only itself, plus atomic counters for density and for damage to whatever it's chewing on.
- **Pathing:** flow fields built with bucketed Dijkstra on background threads: one to the HQ that passes through buildings at a high cost (so the horde chews through when there's no way around), and "hunt" fields toward whatever the faction can see. Diagonal steps between two blocked cells are refused, so a wall line with touching corners seals a street.
- **Rendering:** the city is baked into vertex meshes (roads, water, parks, 2.5D buildings), rebuilt only when something is destroyed. The horde is one map-sized texture refreshed from the density grid, with a soft glow under it that grows with the crowd.
- **Real maps:** type a city in the menu. The game looks it up (Nominatim, then Photon), finds the nearest airport for your HQ and downloads streets, buildings, water and parks from OpenStreetMap through several Overpass mirrors at 4 m per cell, all over its own HTTPS.

The original browser version is kept for reference in `web/`.

## Roadmap

- Multiplayer
- Factorio-style production chains
- More units and abilities
