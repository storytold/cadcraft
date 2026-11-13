# Storeys and the blueprint

Drawings often come as a stack: the floors of a building, the versions of a product (last
year's model beneath, this year's drawn over it), variants of one layout, or simply levels where
material is kept that is needed now and then. CADCraft keeps such a stack apart as **storeys**,
the way architectural CAD does with floors, without a building model: everything stays 2D
drafting. Any storey can lie beneath the one being drawn as a **blueprint**, to trace over and
snap to.

The name comes from buildings; nothing else is assumed. A storey is a level of the drawing,
whatever it holds.

## Two independent switches: layers and storeys

- A **layer** is a trade that runs through the whole building: walls, plumbing, furniture,
  dimensions. Turning a layer off hides it **on every storey**.
- A **storey** is a property of each object. You always work on one storey, the **current** one:
  only its objects are shown, selected and changed.

An object shows when both allow it. "Plumbing off" means no plumbing, whichever storey you are on.

## The blueprint (ghost storey)

Any other storey can lie beneath the current one as a **blueprint**: drawn in one colour (a light beige
unless you choose another), under everything on the current storey. You can **snap** to it, and
nothing else: it is never selected, picked or changed, by a command, a grip or a script. The
ghost follows the layer switches too. It is not plotted and not shown through layout viewports.
Which storey is the blueprint lasts for the session; it is not saved.

## Working with storeys

The **Storeys** palette (docked between Layers and Properties; Window ▸ Storeys Palette brings it
back) lists the storeys from top to bottom, each with its **number**: the start storey is 0, the
storeys above it count 1, 2, 3, those below -1, -2. A name is yours ("2.OG", "Roof",
"Version 2019"); the number is not, it follows the order.

- **Click** a storey to go to it.
- **Double-click** a name to rename it in place (Enter keeps it, Esc goes back).
- The **blueprint mark** in front of a storey is lit on the storey drawn beneath; click it on
  another storey to show that one instead, or on the lit one to hide it.
- **Right-click** a storey: go to it, show or hide it as the blueprint, make it the start storey,
  rename it, add a storey above or below it (named at once), move it up or down, delete it.
- **Settings…** opens the storey settings (Options ▸ Storeys), with the blueprint colours for a dark
  and a light drawing area.

Wherever a storey is asked for by name, its number works too (`STOREY` then `-1`).

### Start setup

Options ▸ Storeys holds the **start setup**: the storeys a new drawing starts with, top first,
and the start storey among them. A new drawing opens on its start storey; a drawing without
storeys (an old one, another office's) gets the setup when it is opened, everything in it on the
start storey; a drawing with storeys opens on its own start storey. Two setups are ready:

| Setup | Storeys (top first) | Start (0) |
|---|---|---|
| Building | DG, 2.OG, 1.OG, EG, 1.UG, 2.UG | EG |
| Neutral | Drawing 2, Drawing 1, Drawing, Blueprint 1, Blueprint 2, Blueprint 3 | Drawing |

Clear "New drawings, and opened drawings without storeys, get these storeys" to work without storeys unless you make them. "Apply to
This Drawing" gives the open drawing the setup when it has no storeys yet.

**Shortcuts are yours**, like every command's: Storey Above (Ctrl+PageUp), Storey Below
(Ctrl+PageDown), Blueprint On/Off (Ctrl+Shift+G) and Choose Blueprint… (Ctrl+G) come with one, Go
to Storey… has none. Change them in Tools ▸ Customize ▸ Keyboard Shortcuts… (Options ▸ Storeys
has a button for it); a key you give a storey command wins over another command's, and the
dialog says which command lost it. On the number keys, for instance: Ctrl+4 go to storey, Ctrl+5
down, Ctrl+6 up, Ctrl+7 choose blueprint, Ctrl+0 blueprint on/off.
Blueprint on/off works while a command runs.

| Command | What it does |
|---|---|
| `STOREY` | Lists the storeys. Typed: a storey's name makes it current; New, Rename, Delete, Ghost |
| `storey.new {name, below?, current?}` | A new storey above (or below) the current one. **The first storey takes everything drawn so far.** |
| `storey.current {name}`, `storey.up`, `storey.down` | Go to a storey (by name or number; the selection is cleared) |
| `storey.home {name}` | The start storey: number 0, the drawing opens on it |
| `storey.template {names?, start?}` | Storeys for a drawing without them (default: the start setup), everything on the start storey |
| `storey.ghost {name \| null, color?: "#rrggbb"}` | The blueprint and its colour |
| `storey.ghost.toggle`, `storey.ghost.up`, `storey.ghost.down` | Blueprint off and on again (the last one, else the storey below); the next storey up or down as the blueprint |
| `storey.rename {name, newName}`, `storey.order {name, index}` | Rename, reorder (index 0 is the bottom) |
| `storey.delete {name}` | Delete an empty storey; deleting the last one turns storeys off |
| `STOREYCOPY {handles?, to: name \| [names]}` | Copy objects to other storeys, in place |
| `STOREYMOVE {handles?, to}` | Move objects to another storey |
| `storey.import {file, name?, ghost?}` | Another office's plan (DXF/DWG) as a new storey at its own coordinates, with its own layers, shown as the ghost |

Undo takes back edits, never the storey you are looking at.

## What lands where

- Everything you draw, paste, insert or explode lands on the current storey. A pasted object
  lands where you paste it, not where it was copied.
- Objects inside blocks and in paper space have no storey; a block shows whole wherever its
  reference is.
- **Layers and block definitions belong to the whole building.** Commands that act on the
  drawing's tables reach every storey, as they should: renaming, merging or deleting a layer
  (LAYDEL deletes that layer's objects on every storey), editing a block (every reference shows
  the new definition, on every storey), styles, AUDIT. Any other command that would change an
  object of another storey is taken back with a message.
- **No links between storeys.** A dimension measured from the ghost is not associative to it;
  STOREYCOPY links the copies to each other (a copied dimension follows its copied line) and drops
  links to anything not copied; STOREYMOVE drops links and constraints that would cross storeys.

## In files

The storey list and the current storey are CADCraft's `CADCRAFT_STOREYS` XRECORD; each object's
storey is its xdata of the registered application `CADCRAFT_STOREY`. Both travel through DXF and
DWG. Other programs keep them and show every storey at once; objects they add have no storey and
join the current storey when the drawing is opened in CADCraft again.

`storey.import` brings the plan's blocks along; a block whose name the drawing already uses comes
under a name of its own (`DOOR$IMPORT1`), so the other office's DOOR and yours stay apart.

## Not yet

- A storey per layout viewport (a sheet with two storeys side by side): viewports show the
  current storey.
- Snapping inside an active layout viewport can still find the ghost, though it is not drawn there.
- Objects copied in another program from one storeys drawing into another keep their storey id,
  which may name a different storey there.
