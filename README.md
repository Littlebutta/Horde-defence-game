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
| 1 to 7, O, L | Wall, gun turret, cannon, scrap extractor, ammo plant, missile silo, evac point, outpost, research lab |
| 8 / 9 | Rifle squad / tank, deployed from the HQ to where you click |
| Left click / drag | Place. Drag walls along a street; drag other buildings to lay a row or grid |
| Left click a unit, right click the map | Select troops, then move them (rifles can garrison a building or man a bunker) |
| R / Q / G / E | Rescue squad / airstrike / gunship / ICBM (panel on the right, greyed out until ready) |
| Q, then right-drag | Airstrike: right-click where the B-2's run starts, drag to its end, let go to send it |
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

- Every match starts by choosing where the HQ goes: the suggested spot is shown first, click anywhere on dry land to move it, then **Start here** (or Enter). An HQ next to the airport takes the whole airport into its base zone. In co-op the host picks.
- Turrets cost more the further they are from the HQ; the extra is shown at the cursor. Outside the base zone they are flown in by helicopter.
- Click a rooftop with a turret selected to mount it there for +60% range. If the building falls, so does the turret.
- Outposts push the base zone outwards. Factories, silos, pads and labs must go inside the zone.
- Rescue squads walk up to civilians one by one and escort them back. Once inside the base, civilians walk to the nearest extraction pad. Each pad flies up to 16 out (+100 score, +20 scrap, +10 ammo each), then needs 30 seconds, so more pads means faster extraction. A crowded base slows your income.
- Every missile silo has its own ICBM cooldown. Nothing survives the inner third of the blast; it weakens out to the edge, where the toughest of the horde live through it.
- **Bunkers** (-) hold any number of riflemen, who fire from inside with +60% range. Squads sent into the same bunker or building join up into one. If it falls, three in four of the troops inside die and the rest are left in the rubble.
- Rifle squads spread out around the spot you send them to; riflemen and tanks need no ammo. Tanks and cannons aim for the thickest part of the horde.
- Special towers: the **flame bunker** (B) is very tough but short ranged: it sprays fire for a second from its walls outward, leaves burning napalm, and won't hit a spot again until the napalm there is out; the **railgun** (N) picks the line through the most of the horde (leading their movement), charges, then kills everything in a wide straight line to the end of its range; the **helium tower** (I) slows everything in its field to 40% speed and needs no ammo; **artillery** (Y) shells any spot on the map (click it, right-click a target, **Cease fire** to stop) at 45 ammo a shell.
- The **heli pad** (U) keeps its own gunship: click the pad, right-click anywhere and it flies there for free, fires for 20 s (right-click again to move it), then rearms for 35 s.
- Storage is limited (2500 scrap, 1500 ammo to start, shown as stored/limit in the top bar): each **scrap depot** (0) adds 2500 scrap, each **ammo depot** (V) 2000 ammo.
- Gun turrets never pile onto the same target, and a rooftop turret needs a roof big enough for its whole base.
- Civilians queuing at a pad that is turning around walk over to an idle pad.
- A **rescue station** (J) sends a rescue team every 25 s to the biggest group of civilians it can reach safely and walks them back to the base.
- The research lab unlocks upgrades: bigger rescue squads, carpet bombing (drag to aim the bomb run), a nuclear warhead that leaves radiation nobody of yours can enter or build in, and (on its own panel under the abilities) reinforced armour for walls and turrets, shell presses for faster ammo, long barrels for turret range, and turret mounts (a turret on top of a bunker gets the rooftop range bonus).
- Units go anywhere on land except inside buildings: water is the only barrier. Only your turrets and garrisoned rifles stand on rooftops.
- Hordes walk around your walls and only chew through one when it blocks the way.
- Real cities you have played appear under **Played before** on the menu and start instantly, without downloading again (the x removes one).

## World campaign

Pick **World campaign** on the menu. The outbreak is spreading through the world's biggest cities and you're the one the governments send in. It starts in **New York**; after that you pick each city on a globe. Every campaign draws ten more from a pool of forty well-known cities (London, Paris, Tokyo, Rio de Janeiro, Cairo, Sydney, Moscow, Mumbai, Lagos and the rest), at least 1,000 km apart, so no two campaigns visit the same places.

- The globe between cities: drag to turn it, scroll to zoom, click a city (on the globe or in the list) to pick it. Every city shows its **days until it falls** and the **people still alive** there, from its real metro population.
- Every city you fight in takes 10 days. Meanwhile the others count down, each at its own speed, and fewer people are left as a city nears its end. A city that reaches zero falls to the horde and can't be picked any more.
- The closer a city is to falling, the bigger its horde: from ×0.8 for a quiet city to ×3.2 for one about to fall, plus 6% for every city fought before. Civilians on the map scale with the people still alive (about one per 1,700).
- Hold each city until the evac timer runs out (7 minutes, 20 seconds longer for every city fought, at most 11).
- The maps are the city centres. The suggested HQ spot is an airport only when one is close enough to share the map with downtown; otherwise it's the middle of the city. You can move it before the city starts.
- Every civilian flown out of a held city is a **research point**. Between cities you spend them on the **tech tree** or on recruiting troops. You start with only walls, the scrap extractor, the evac point, the rescue station and rifles; every turret, the tank, every ability and the lab's research must be unlocked. Unlocking your first turret brings the ammo plant with it, and every turret has two upgrades of its own (faster fire, armour piercing, napalm, wider rail beams and so on). There is no research lab inside campaign cities.
- A lost city (the HQ overrun) pays nothing, but the campaign goes on. Lose **three** and the world's governments fire you: the campaign is over.
- Troops that survive (rifle squads and tanks, with however many soldiers are left) march on to the next city and roll out of the HQ when it starts. Scrap, ammo and buildings stay behind.
- Progress saves to `data/campaign.txt` after every city and every purchase; **Continue** picks it up.
- The maps are downloaded from OpenStreetMap ahead of time by the **Download campaign cities** workflow and published as the `cities` release. The game fetches the current campaign's maps from GitHub in the background, three at once and the city you picked first (into `data/campaign`), so no campaign city waits on OpenStreetMap. A map that's missing anyway downloads live like any real city: four map tiles at a time from different OpenStreetMap servers.
- The globe's coastlines are Natural Earth's 1:50m land (public domain), baked into `assets/land.bin` by `tools/bake_land.py`.
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
- **Orcs:** portals pour out huge waves that destroy everything they see. Orcs build rage as units around them die, turning redder and running faster. When one falls, the orcs around it pull back, bunch up, and charge in again together at full rage. They sometimes grab civilians and drag them back through a portal (troops nearby can free them); every capture brings more orcs out at full rage.
- **Chaos:** all three armies at once, each a little smaller than on its own and each in its own colour: zombie outbreaks, bug drops and hives, and orc portals.

## Building from source

```
cargo build --release
```

The exe is at `target/release/PixelHorde.exe` (or `PixelHorde` on Linux/macOS). Builds without `BUILD_NUMBER` set report themselves as development builds and don't self-update.

Steam support is the default `steam` feature; `cargo build --release --no-default-features` builds without it. The Steam API DLL ships inside the exe and is written next to it on first start.

For headless testing on Linux: `PH_SHOT=out.png PH_FAC=1 PH_TICKS=2000 xvfb-run ./target/release/PixelHorde` runs a random city for 2000 ticks and saves a screenshot. Add `PH_STRESS=1` to drop a million units on the map, `PH_CAMP=5` to play campaign city 5 (Cairo; 0 is New York) with a small veteran army (its real map if it is in `data/campaign`, else a random one), `PH_HUB=1` to show the campaign screen, or `PH_SCRIPT=1` to build a base and fire every ability. `PH_ZOOM`, `PH_YAW` and `PH_PITCH` set the camera, and `PH_PROF=1` prints where each tick's time goes. For co-op without Steam, `PH_HOST=47777` and `PH_JOIN=127.0.0.1:47777` connect two copies over TCP, and `PH_NETTEST=1000` runs a scripted co-op game for 1000 ticks and prints both sides' checksums. `cargo test --release` checks that two games given the same commands stay identical.

## How it works

- **Swarm:** units live in one flat array that rayon splits across all cores each tick. Every unit reads shared grids (open streets, flow fields, last tick's crowd density, queued damage) and writes only itself, plus atomic counters for density and for damage to whatever it's chewing on.
- **Pathing:** flow fields built with bucketed Dijkstra on background threads: one to the HQ that passes through buildings at a high cost (so the horde chews through when there's no way around), and "hunt" fields toward whatever the faction can see. Diagonal steps between two blocked cells are refused, so a wall line with touching corners seals a street.
- **Rendering:** fully 3D. Roads, water and parks are baked into flat meshes; buildings are 3D blocks baked per map tile, so a destroyed building only rebuilds its own tile. Clicks are ray-cast into the scene, so rooftops can be picked from any angle. The horde is one map-sized texture refreshed from the density grid, with a soft glow under it that grows with the crowd.
- **Real maps:** type a city in the menu. The game looks it up (Nominatim, then Photon), finds the nearest airport for your HQ and downloads streets, buildings, water and parks from OpenStreetMap through several Overpass mirrors at 4 m per cell, all over its own HTTPS.

The original browser version is kept for reference in `web/`.

## Roadmap

- Factorio-style production chains
- More units and abilities
