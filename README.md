# Pixel Horde

A horde defence game where every enemy is a single pixel. Hundreds of thousands to millions of zombies, alien bugs and orcs pour through a city's streets like water while you build walls, turrets, an economy and missile strikes to hold your HQ.

## Play on Windows (updates itself)

1. Install [Git for Windows](https://git-scm.com/download/win) if you don't have it.
2. Clone this repo once:
   ```
   git clone https://github.com/Littlebutta/Horde-defence-game.git
   ```
3. Double-click `Play.bat` in the cloned folder.

`Play.bat` pulls the latest commit from GitHub, starts a small local server and opens the game in your browser. While the window stays open it checks GitHub every minute. When a new build lands, the game reloads itself if no wave has started yet; mid-game a green "Update downloaded: reload" button appears so you don't lose a run.

You can also open `game/index.html` directly in a browser, but then it won't update itself.

## Controls

| Input | Action |
| --- | --- |
| 1 to 6 | Wall, gun turret, cannon, scrap extractor, ammo plant, missile silo |
| Q / E | Airstrike / ICBM (needs a silo) |
| X | Sell a structure for 50% |
| Left click / drag | Place (drag to paint walls) |
| Right drag, WASD | Pan |
| Scroll | Zoom |
| Space | Pause |

Turrets placed fully on a rooftop get +60% range and can't be reached by the horde.

## How it works

- **Threads:** the horde is split across Web Workers, one per spare CPU thread. Each worker owns its units outright; only density grids and damage events cross threads, so the cost per unit stays tiny.
- **Pathing:** one flow field (bucketed Dijkstra from the HQ) is shared by every unit. Units step to the cheapest neighbour cell, with a crowding penalty that makes them spill around each other like a fluid. Player structures are passable at a high cost, so the horde goes around walls when it can and chews through them when it can't.
- **Rendering:** one pixel per map cell, coloured by faction and brightened by crowd density.
- **Economy:** extractors make scrap, ammo plants turn scrap into ammo, turrets burn ammo.

## Roadmap

- Real-world city maps from OpenStreetMap (type an address or city name)
- Multiplayer
- Factorio-style production chains for ammo and parts
- Vehicles, garrisoning units in buildings, more abilities
- Distinct faction behaviours (bugs swarming walls, orcs smashing buildings)
- GPU compute and SharedArrayBuffer for tens of millions of units
