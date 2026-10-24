# Pixel Horde

A horde defence game where every enemy is a single pixel. Up to millions of zombies, alien bugs or orcs pour through a city's streets like water while you hold an airport with walls, turrets, an economy, rescue squads, troops and missile strikes.

It is a native Windows game written in Rust. The horde is simulated in parallel on every CPU core and drawn on the GPU.

## Play on Windows

1. Open the [latest release](https://github.com/Littlebutta/Horde-defence-game/releases/latest) and download `PixelHorde.exe`.
2. Put it in any folder and double-click it. Windows SmartScreen may warn about an unknown app the first time: click **More info**, then **Run anyway**.

That's all. No Git, browser or installer needed. The game keeps everything it saves in a `data` folder next to the exe: your settings and every real city you have downloaded, so you can move or back up the whole folder. Every time the game starts it checks this repo's releases; when a newer build exists it downloads it in the background and a **Restart now** button appears on the menu.

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
| Left drag on empty map, WASD | Pan |
| Middle drag, or Alt + left drag | Orbit the camera (sideways turns, up and down tilts) |
| Z / C | Turn the camera |
| Page Up / Page Down | Tilt the camera |
| Scroll | Zoom towards the cursor |
| T | Top-down or 3D view |
| K | Dark or light map (dark is the default) |
| H / F | Jump to HQ / fit the whole map |
| Space | Pause |

- Turrets cost more the further they are from the HQ; the extra is shown at the cursor. Outside the base zone they are flown in by helicopter.
- Click a rooftop with a turret selected to mount it there for +60% range. If the building falls, so does the turret.
- Outposts push the base zone outwards. Factories, silos, pads and labs must go inside the zone.
- Rescue squads walk up to civilians one by one and escort them back. Once inside the base, civilians walk to the nearest extraction pad. Each pad flies up to 16 out (+100 score, +20 scrap, +10 ammo each), then needs 30 seconds, so more pads means faster extraction. A crowded base slows your income.
- Every missile silo has its own ICBM cooldown.
- The research lab unlocks upgrades: bigger rescue squads, carpet bombing (drag to aim the bomb run), a nuclear warhead that leaves radiation nobody of yours can enter or build in, and (on its own panel under the abilities) reinforced armour for walls and turrets, shell presses for faster ammo, and long barrels for turret range.
- Units go anywhere on land except inside buildings: water is the only barrier. Only your turrets and garrisoned rifles stand on rooftops.
- Hordes walk around your walls and only chew through one when it blocks the way.
- Real cities you have played appear under **Played before** on the menu and start instantly, without downloading again (the x removes one).

## World campaign

Pick **World campaign** on the menu to fight through ten of the world's biggest cities, smallest first: Osaka, Dhaka, Beijing, Mumbai, Cairo, Mexico City, São Paulo, Shanghai, Delhi and Tokyo.

- The maps are the city centres. Megacity airports are mostly far out of town, so the HQ only goes at an airport when one is close enough to share the map with downtown; otherwise it sits in the middle of the city.
- Hold each city until the evac timer runs out (7 minutes in Osaka, 30 seconds longer every city). The horde is 30% bigger in every city, so Tokyo has about 10 times Osaka's.
- Troops that survive (rifle squads and tanks, with however many soldiers are left) march on to the next city and roll out of the HQ when it starts. Scrap, ammo and buildings stay behind: every city starts from the same small base.
- Civilians you fly out are the campaign's currency. Between cities you spend them on the research the lab sells for scrap in a normal game (Bigger squads, Carpet bombing, Nuclear warhead, Reinforced armour, Shell presses, Long barrels) or on recruiting more troops. There is no research lab inside campaign cities.
- If the HQ falls, try the city again with the army and civilians you had before it.
- Progress saves to `data/campaign.txt` after every city and every purchase; **Continue** picks it up.
- The ten maps are downloaded from OpenStreetMap ahead of time by the **Download campaign cities** workflow and published as the `cities` release. The game fetches them from GitHub in the background when it starts (into `data/campaign`), so no campaign city waits on OpenStreetMap. A map that's missing anyway downloads live like any real city.
- The campaign is single player for now.

## Co-op with Steam friends

Up to 4 players share one base: the same scrap, buildings and troops, and everyone can build, deploy and fire abilities.

1. Everyone starts Steam, then starts `PixelHorde.exe`.
2. The host clicks **Play co-op with Steam friends**. The Steam invite window opens; invite your friends.
3. Friends accept the invite in Steam (or right-click the host in the friends list and pick **Join game**) while Pixel Horde is open. They see a waiting room.
4. The host picks the faction, map and difficulty and presses **Start**. The map is sent to everyone, so real cities only need downloading on the host.

Everybody needs the same build; the auto-updater keeps that true, so restart if the game says the versions differ. For now the game uses Steam's free test app (Spacewar, App ID 480), so Steam shows you as playing Spacewar, and an invite accepted while Pixel Horde is closed launches Spacewar instead. Open Pixel Horde first, then accept.

How it stays in sync: every player runs the same deterministic simulation and only commands go over the network (lockstep, like Factorio). The host bundles everyone's commands into a frame for each tick a few ticks ahead; nobody simulates a tick before they have its frame. Every 100 ticks the games compare checksums, and you get a warning if one ever drifts.

## Factions

- **Zombies:** small outbreaks that grow by eating civilians; every bite turns one civilian into two zombies. A zombie that dies gets back up once. When one is killed, the zombies around it speed up and swarm whatever killed it, and stay locked on until it's dead. They wander, attack what they see, and periodically rush the HQ.
- **Alien bugs:** small drops all over the city, straight for the HQ. Some drops are lighter-coloured flyers that cross rooftops and walls. Hives take root now and then: they spawn bugs, stop you building nearby, grow, and infect buildings around them, each one making the hive's waves bigger. Turrets can't hurt a hive; use abilities or troops.
- **Orcs:** portals pour out huge waves that destroy everything they see. Orcs build rage as units around them die, turning redder and running faster. They sometimes grab civilians and drag them back through a portal (troops nearby can free them); every capture brings more orcs out at full rage.

## Building from source

```
cargo build --release
```

The exe is at `target/release/PixelHorde.exe` (or `PixelHorde` on Linux/macOS). Builds without `BUILD_NUMBER` set report themselves as development builds and don't self-update.

Steam support is the default `steam` feature; `cargo build --release --no-default-features` builds without it. The Steam API DLL ships inside the exe and is written next to it on first start.

For headless testing on Linux: `PH_SHOT=out.png PH_FAC=1 PH_TICKS=2000 xvfb-run ./target/release/PixelHorde` runs a random city for 2000 ticks and saves a screenshot. Add `PH_STRESS=1` to drop a million units on the map, `PH_CAMP=3` to play campaign city 4 with a small veteran army (on a random map), `PH_HUB=1` to show the campaign screen, or `PH_SCRIPT=1` to build a base and fire every ability. `PH_ZOOM`, `PH_YAW` and `PH_PITCH` set the camera, and `PH_PROF=1` prints where each tick's time goes. For co-op without Steam, `PH_HOST=47777` and `PH_JOIN=127.0.0.1:47777` connect two copies over TCP, and `PH_NETTEST=1000` runs a scripted co-op game for 1000 ticks and prints both sides' checksums. `cargo test --release` checks that two games given the same commands stay identical.

## How it works

- **Swarm:** units live in one flat array that rayon splits across all cores each tick. Every unit reads shared grids (open streets, flow fields, last tick's crowd density, queued damage) and writes only itself, plus atomic counters for density and for damage to whatever it's chewing on.
- **Pathing:** flow fields built with bucketed Dijkstra on background threads: one to the HQ that passes through buildings at a high cost (so the horde chews through when there's no way around), and "hunt" fields toward whatever the faction can see. Diagonal steps between two blocked cells are refused, so a wall line with touching corners seals a street.
- **Rendering:** fully 3D. Roads, water and parks are baked into flat meshes; buildings are 3D blocks baked per map tile, so a destroyed building only rebuilds its own tile. Clicks are ray-cast into the scene, so rooftops can be picked from any angle. The horde is one map-sized texture refreshed from the density grid, with a soft glow under it that grows with the crowd.
- **Real maps:** type a city in the menu. The game looks it up (Nominatim, then Photon), finds the nearest airport for your HQ and downloads streets, buildings, water and parks from OpenStreetMap through several Overpass mirrors at 4 m per cell, all over its own HTTPS.

The original browser version is kept for reference in `web/`.

## Roadmap

- Factorio-style production chains
- More units and abilities
