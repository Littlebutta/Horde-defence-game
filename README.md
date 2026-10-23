# Pixel Horde

A horde defence game where every enemy is a single pixel. Hundreds of thousands to millions of zombies, alien bugs or orcs pour through a city's streets like water while you hold an airport with walls, turrets, an economy, rescue squads and missile strikes.

Pick one faction per game and either a generated city or a real one: type a city or address and the map is pulled from OpenStreetMap, with your HQ placed at the nearest airport.

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
| 1 to 7 | Wall, gun turret, cannon, scrap extractor, ammo plant, missile silo, extraction pad |
| 8 / 9 | Rifle squad / tank, deployed from the HQ to where you click |
| Left click / drag | Place. Drag walls along a street; drag other buildings to lay a row or grid |
| R / Q / E | Rescue squad / airstrike / ICBM (the panel on the right; greyed out until ready) |
| X | Sell a structure for 50% |
| Right drag, WASD | Pan |
| Scroll | Zoom |
| T | Switch between 2.5D and top-down |
| H / F | Jump to HQ / fit the whole map |
| Space | Pause |

- Click a rooftop with a turret selected to mount it there: +60% range, but it falls with the building. Rifle squads sent to a building garrison it the same way.
- Turrets placed outside the base zone (the dashed ring) are flown in from the HQ by helicopter.
- Factories, silos and extraction pads must go inside the base zone.
- Rescued civilians wait at the HQ until an extraction pad flies them out: +100 score, +20 scrap and +10 ammo each.
- Hordes and civilians only move along streets, plazas, parks, airport ground and rubble.

## Factions

- **Zombies:** a few small outbreaks that grow by eating civilians, who rise as new zombies. They attack anything in sight, wander otherwise, and periodically rush the HQ all at once. After four rushes they never stop.
- **Alien bugs:** small drops land all over the city every few seconds. They head straight for the HQ and kill civilians in their way.
- **Orcs:** one to three portals open at a time and pour out huge waves that destroy everything they see.

## How it works

- **Threads:** the horde is split across Web Workers, one per spare CPU thread. Each worker owns its units outright; only density grids and damage events cross threads, so the cost per unit stays tiny.
- **Pathing:** one flow field (bucketed Dijkstra from the HQ) is shared by every unit. Units step to the cheapest neighbour cell, with a crowding penalty that makes them spill around each other like a fluid. Player structures are passable at a high cost, so the horde goes around walls when it can and chews through them when it can't.
- **Rendering:** one pixel per map cell, darker where the crowd is denser, plus a glow that grows with swarm size. Streets and buildings are drawn as vectors in a Google Maps style; buildings are extruded to their height (2.5D) and cached in tiles that redraw when something is destroyed.
- **Destruction:** every city building has hit points. The horde chews through buildings and walls when there's no way around, and big explosions flatten them, which re-routes the flow field.
- **Real maps:** when the game runs through `Play.bat`, the launcher downloads map data on the game's behalf (browsers often block direct requests). Nominatim finds the place, Overpass downloads streets, buildings, water, parks and airport runways for a 4 km square, and the same rasteriser used for generated cities turns it into the battlefield. Street widths are exaggerated so the swarm has room to flow.
- **Economy:** extractors make scrap, ammo plants turn scrap into ammo, turrets burn ammo.

## Roadmap

- Multiplayer
- Factorio-style production chains for ammo and parts
- Vehicles, garrisoning units in buildings, more abilities
- Distinct faction behaviours (bugs swarming walls, orcs smashing buildings)
- GPU compute and SharedArrayBuffer for tens of millions of units
