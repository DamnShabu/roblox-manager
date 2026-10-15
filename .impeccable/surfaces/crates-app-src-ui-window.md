---
version: 1
slug: "crates-app-src-ui-window"
primary_target: "crates/app/src/ui/window"
related_targets: []
---

# Main window

Scope: the main window of Stacked (crates/app/src/ui/window and the table,
sidebar, details pane and status bar it builds). Mode: Operate.

Audience and job: a Linux player running a main plus many alts (31 accounts,
about 10 running) who comes back to the window between other things to see
what is running, fix what needs attention, change performance and run or stop
macros, then leaves it.

Constraints: function first (PRODUCT.md); today's dark system, refined, not
replaced; Inter bundled; Material Symbols Rounded 400 for every icon,
checkboxes included; accounts stay a dense table; groups stay the main unit;
leader/auto-join is gone from the window, the main account is pinned instead;
the multi-client risk line stays visible; benchmarks are Roblox Account
Manager and Mission Center.

Approved sketch: .impeccable/mocks/decision/r7-assigned.html (the user locked
it after asking for "more premium, more solid and reliable, not slop").

## Direction contract

THESIS: One working window where state is readable at a glance and every
control for the selection is one click away. It refuses the category default
of a launcher grid with settings hidden in menus and dialogs.

OWN-WORLD: Near-black content (#0f1013) with a lifted side layer for the
header, sidebar and status bar, 1px hairlines, no shadows on resting
surfaces, no coloured edge stripes. Red (#ff3d2e) only on the primary Launch;
"on" is white (checkboxes, switch, selected segment); state colour lives in
small dots and icons (green running, amber joining, coral problems). Inter at
14/13/12 with sentence-case labels; Material Symbols Rounded throughout; 6px
corners; real game icons and account avatars.

STORY: The user sees within a second what runs (counts in the sidebar, a dot
and word per row), what needs them (a Needs attention section at the top
with each fix inline), and what the machine is carrying (CPU and memory per
client, totals per group, machine load in the sidebar). They change
performance from the row or for a whole selection, run and stop macros from
the row, the sidebar or the toolbar, and launch the selection into the target
beside the Launch button.

FIRST VIEWPORT: Header bar (52px): mark, "Stacked", a muted status line,
search with its shortcut, add, refresh, menu. Left sidebar (240px): Status
filters with counts (Needs attention in coral), Groups with their game icons
and running counts, Macros with run or stop and where they play, Machine at
the bottom. Main column: toolbar with the view title and count, the
selection's Performance, Run macro, Move to and Stop, then Stop all, the
launch target and the red "Launch N selected"; the table header; Needs
attention rows with reason and fix; the pinned main account with a star;
each group as a row with its game, running count, CPU and memory totals and
Stop or Launch; account rows with avatar, name, @user, status, uptime as
"2 h 14 m", CPU, memory, macro with round or step, a quiet performance
select with a relaunch marker when the running client differs, and hide,
stop and more. Under the table the details pane (about 210px, foldable):
the selected account's summary and tabs, then Performance (segmented, with
what it does and that it applies next launch), Macro (picker, Stop or Run,
Edit, live step with progress), Window (Hide, Macro-ready switch), Session
(status, Check now, Move to). A 30px status bar carries the risk line, the
last activity and the Activity drawer toggle.

FORM: Filters and details (sidebar filters + table + details pane), position
3 of the 7 structures ranked for this surface, dealt as the lead; surface
seed key 99715a0b; visual world: the incumbent system refined (direction
seed ecb6304a, resolved to the standing exit).

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

## Unresolved

- Sorting by column (the sketch shows Status sorted) may land after the
  first build.
- Per-client CPU and memory are new readings the core does not take yet.
- The rename to Stacked beyond the main window (About, packaging, README) is
  separate work.
