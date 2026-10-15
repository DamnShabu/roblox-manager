---
name: Stacked
description: One dark working window for running many Roblox clients at once; state at a glance, every control one click away.
colors:
  bg: "#0f1013"
  side: "#14151a"
  panel: "#16171c"
  raised: "#1e2026"
  raised-2: "#2a2d35"
  border: "#23252c"
  border-strong: "#30333b"
  text: "#eceef2"
  text-2: "#a3a7b2"
  text-3: "#7d818c"
  accent: "#ff3d2e"
  accent-hover: "#ff5a4e"
  on-accent: "#140806"
  success: "#4cc38a"
  warning: "#f0a641"
  danger: "#ff7a70"
typography:
  headline:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 600
    letterSpacing: "-0.01em"
  view-title:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "17px"
    fontWeight: 600
    letterSpacing: "-0.012em"
  title:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 600
  title-sm:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 600
  label:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 500
  body:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 400
  body-sm:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
  control:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 500
  caption:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 500
  section-head:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "12px"
    fontWeight: 600
  numeral:
    fontFamily: "Inter Variable, Inter, Cantarell, system-ui, sans-serif"
    fontSize: "13px"
    fontWeight: 400
    fontFeature: "\"tnum\""
  mono:
    fontFamily: "JetBrains Mono, monospace"
    fontSize: "12px"
    fontWeight: 400
rounded:
  xs: "4px"
  sm: "5px"
  md: "6px"
  seg: "7px"
  lg: "8px"
  xl: "10px"
  full: "9999px"
spacing:
  "4": "4px"
  "6": "6px"
  "8": "8px"
  "10": "10px"
  "12": "12px"
  "16": "16px"
  "24": "24px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    typography: "{typography.control}"
    rounded: "{rounded.md}"
    padding: "0 14px"
    height: "32px"
  button-primary-hover:
    backgroundColor: "{colors.accent-hover}"
  button-secondary:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.text}"
    typography: "{typography.control}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "32px"
  button-secondary-hover:
    backgroundColor: "{colors.raised-2}"
  button-secondary-sm:
    padding: "0 9px"
    height: "28px"
  button-ghost:
    textColor: "{colors.text}"
    typography: "{typography.control}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "32px"
  button-ghost-hover:
    backgroundColor: "{colors.raised}"
  button-danger:
    backgroundColor: "{colors.danger}"
    textColor: "{colors.on-accent}"
    rounded: "{rounded.md}"
    height: "32px"
  icon-button:
    textColor: "{colors.text-2}"
    rounded: "{rounded.md}"
    size: "32px"
  icon-button-hover:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.text}"
  icon-button-on:
    backgroundColor: "{colors.raised-2}"
    textColor: "{colors.text}"
  input:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.text}"
    typography: "{typography.body-sm}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "32px"
  segmented:
    backgroundColor: "{colors.bg}"
    rounded: "{rounded.seg}"
    padding: "2px"
  segment:
    textColor: "{colors.text-2}"
    typography: "{typography.control}"
    rounded: "{rounded.sm}"
    padding: "0 13px"
    height: "26px"
  segment-on:
    backgroundColor: "{colors.raised-2}"
    textColor: "{colors.text}"
  switch:
    backgroundColor: "{colors.raised-2}"
    rounded: "{rounded.full}"
    width: "32px"
    height: "18px"
  switch-on:
    backgroundColor: "{colors.text}"
  checkbox:
    textColor: "{colors.text-3}"
    size: "18px"
  checkbox-on:
    textColor: "{colors.text}"
  tab:
    textColor: "{colors.text-3}"
    typography: "{typography.control}"
    padding: "0 12px"
  tab-on:
    textColor: "{colors.text}"
  header-bar:
    backgroundColor: "{colors.side}"
    textColor: "{colors.text}"
    padding: "0 10px 0 16px"
    height: "52px"
  sidebar:
    backgroundColor: "{colors.side}"
    textColor: "{colors.text}"
    width: "240px"
  sidebar-item:
    textColor: "{colors.text}"
    typography: "{typography.body}"
    rounded: "{rounded.md}"
    padding: "0 10px"
    height: "32px"
  sidebar-item-on:
    backgroundColor: "{colors.raised}"
  toolbar:
    textColor: "{colors.text}"
    padding: "0 16px"
    height: "56px"
  table-head:
    textColor: "{colors.text-3}"
    typography: "{typography.caption}"
    padding: "0 16px"
    height: "34px"
  table-row:
    backgroundColor: "{colors.bg}"
    textColor: "{colors.text}"
    padding: "0 16px"
    height: "40px"
  table-row-current:
    backgroundColor: "{colors.raised}"
  group-row:
    backgroundColor: "{colors.side}"
    textColor: "{colors.text}"
    padding: "0 16px"
    height: "42px"
  details-pane:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text}"
  status-bar:
    backgroundColor: "{colors.side}"
    textColor: "{colors.text-3}"
    typography: "{typography.caption}"
    padding: "0 12px 0 16px"
    height: "30px"
  badge:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.text}"
    typography: "{typography.caption}"
    rounded: "{rounded.full}"
    padding: "0 8px"
    height: "22px"
  chip:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.text}"
    typography: "{typography.caption}"
    rounded: "{rounded.full}"
    padding: "0 9px"
    height: "24px"
  tooltip:
    backgroundColor: "{colors.raised-2}"
    textColor: "{colors.text}"
    rounded: "{rounded.md}"
  popover:
    backgroundColor: "{colors.panel}"
    textColor: "{colors.text}"
    rounded: "{rounded.xl}"
    padding: "6px"
---

<!-- Recorded from the shipped build of Stacked's main window (style.css :root
tokens, crates/app/src/ui, screenshots app-v7-wide/narrow) on 2026-10-14.
Replaces the v0.10 "Roblox Manager" record. -->

# Design System: Stacked

## Overview

**Creative North Star: "The Control Room"**

One working window, checked between other things and then left. Everything
running is visible at once and quiet until something needs the user; what
needs them arrives with its fix. The ground is near-black and low-glare so
thirty rows of accounts read as one calm surface, and the one red button is
the thing the eye finds first.

It is a native GTK4/libadwaita app, dense and flat. A 52px header bar, a 240px
sidebar of filters, groups, macros and machine load, a main column of toolbar
over a dense accounts table, a foldable details pane under the table, and a
30px status bar with an activity drawer. Structure is drawn with 1px hairlines
and close tonal steps of near-black, not cards or shadows. Colour is spent on
state alone, and only in small dots and icons; "on" is white. The stylesheet
also sets Adwaita's own named colours to the same tokens, and gives every
surface it paints a text colour too, so the desktop's palette never lands on
these surfaces. Dark only.

Function first: nothing on screen is decoration. Real game icons and real
account avatars carry identity; type, hairlines and white do the rest.

**Key Characteristics:**
- Dark only: near-black content, a lifted side layer for header, sidebar and status bar, a panel layer for the details pane and popovers.
- One red, on the primary action and on focus; nothing else is red.
- "On" is white: checked checkboxes, switches, the selected segment's label, the current tab's underline.
- State colour lives only in dots and icons, always beside a word.
- Inter at 14/13/12 with sentence-case labels; tabular numerals for every reading.
- Material Symbols Rounded (weight 400) for every icon, checkboxes included.
- 6px corners on every control.

## Colors

A near-black control room with one hot red signal, white for "on", and three muted state lights.

### Primary
- **Signal Red** (`accent`): the one primary action (the toolbar's Launch) and the 2px focus outline. The same red as the app's mark.
- **Signal Red, lit** (`accent-hover`): the primary action under the pointer.
- **Ember Black** (`on-accent`): text and icons on red and on coral.

### Neutral
- **Control-room Black** (`bg`): the window, the table, its rows, the segmented control's well.
- **Side Layer** (`side`): header bar, sidebar, group rows, status bar. Lifted one step from the content.
- **Panel** (`panel`): the details pane, the inspector, popovers, dialogs, the activity drawer, lists.
- **Raised** (`raised`): control grounds (secondary buttons, inputs, search, the launch-target picker), hover grounds, the current row, the chosen sidebar item, badges, chips.
- **Raised 2** (`raised-2`): hover on Raised, toggled icon buttons, the selected segment, tooltips, the unchecked switch.
- **Hairline** (`border`): every divider and outline at rest.
- **Hairline, Strong** (`border-strong`): the toolbar separator, scrollbars, keycaps, popover rings, the radio ring.
- **Readout White** (`text`): primary text, and every "on" state.
- **Secondary Grey** (`text-2`): readings (uptime, CPU, memory), resting icons in buttons, idle state.
- **Dim Grey** (`text-3`): column heads, section heads, @usernames, placeholders, unchecked checkboxes, the status bar.

### Status
- **Running Green** (`success`): running dots, a playing macro's icon, the signed-in icon.
- **Waiting Amber** (`warning`): launching and joining dots, the relaunch-pending marker, friend activity icons.
- **Fault Coral** (`danger`): failed and expired dots and icons, the Needs attention count, destructive buttons, error log lines. Softer than Signal Red so a fault never reads as the primary action.

Row tints sit between the tokens and are written in place: hover `#15161b`, selected `#16171d`, group-row hover `#17181e`. Coral washes are mixed in place with `color-mix` (8 to 16% into Panel or transparent) for the inline confirm, the recording banner and the Needs attention count.

### Named Rules
**The One Red Rule.** Signal Red is the one primary action and the focus ring. A group's own Launch, Run, Check now and every other button are grey.

**The White Is On Rule.** A checked checkbox, a switch that is on, the chosen segment and the current tab are white. "On" is never red and never a status colour.

**The Dot And Word Rule.** State colour appears only in small dots (7 to 8px) and icons, each beside a word (Idle, Running, Expired). No coloured row grounds, no coloured edge stripes, no coloured text blocks.

## Typography

**Body Font:** Inter Variable (bundled with the app; Inter, Cantarell, system-ui as fallbacks)
**Label/Mono Font:** JetBrains Mono (bundled), for log times, keycaps, macro step fields, codes.

**Character:** a neutral grotesque at small sizes for a dense instrument; weight, not size, separates a name from its reading. Both families ship inside the binary and are added to the font map at start, so every machine draws the design's type.

### Hierarchy
- **Headline** (600, 20px, -0.01em): empty-table titles, the largest panel heads.
- **View title** (600, 17px, -0.012em): the toolbar's view name ("All accounts"), with its count in Dim Grey beside it.
- **Title** (600, 16px): panel heads.
- **Title, small** (600, 15px): the app name in the header (-0.01em), the selected account's name in the details pane.
- **Label** (500, 14px): account names in the table; group names at 600.
- **Body** (400, 14px): the window's base size; sidebar items.
- **Body, small / Control** (400 or 500, 13px): buttons, inputs, menus, @usernames, status words, readings.
- **Caption** (500, 12px): column heads and field labels in Dim Grey, badges, chips, help text at 400.
- **Section head** (600, 12px, Dim Grey, sentence case): sidebar groups ("Status", "Groups", "Macros", "Machine"), menu titles.
- **Mono** (400, 12px): log times, keycaps, step fields; the Quick Login code at 30px, 600, 0.14em.

### Named Rules
**The Sentence Case Rule.** Every label, head, button and column is sentence case. Nothing is upper-cased or letter-spaced for effect.

**The Tabular Reading Rule.** Every number the user compares (counts, uptime, CPU, memory, machine load) uses tabular numerals; CPU and memory sit right-aligned in their columns.

## Layout

A fixed shell around one table. Header bar 52px across the top. Sidebar 240px on the left (padding 6px 10px 12px, section heads 14px over their items, Machine pinned to the foot behind a hairline). The main column: a 56px toolbar (view title and count, a hairline separator, the selection's actions, then Stop all, the launch target and the red Launch at the far right), a 34px table head, the table, then the details pane (a 52px head with the account and its tabs, a field grid at 16px padding, foldable). A 30px status bar runs the full width at the bottom; the activity drawer rises from it.

The table: 40px account rows, 42px group rows, 16px side padding, 12px between columns. Columns have fixed widths (check 20, status 124, uptime 80, CPU 60, memory 76, macro 200, performance 140, actions 84); only the account column expands, so spare width lands between the name and its state and every row shares it out the same way. As the window narrows the columns leave in order: uptime and memory below 1280px, CPU and macro below 1100px, performance below 860px. Group rows span their game picker across the columns that remain.

Spacing is set where a box is built, never in the stylesheet as well (GTK adds the two). Common steps: 4, 6, 8, 12 and 16px; 10px inside controls and the sidebar.

## Elevation & Depth

Flat. Depth is tonal: Control-room Black under the Side Layer and Panel, Raised and Raised 2 on top, split by 1px hairlines. Shadows exist only on what floats over the window; outlines inside the window are drawn as inset 1px rings, not shadows.

### Shadow Vocabulary
- **Float** (`box-shadow: 0 2px 6px rgb(0 0 0 / 40%), 0 12px 32px rgb(0 0 0 / 45%)`): popovers and menus (over a 1px Hairline, Strong ring), toasts, the activity drawer.
- **Segment lift** (`box-shadow: 0 1px 2px rgb(0 0 0 / 40%)`): the selected segment in a segmented control; the one resting shadow, a 1px lift that tells the chosen option from its well.

### Named Rules
**The Hairline Rule.** Regions and rows are separated by a 1px Hairline. Nothing that rests carries a shadow, apart from the segment lift.

## Shapes

Small, even corners. Every control (buttons, icon buttons, inputs, selects, sidebar items, menu items, tooltips) is 6px. The segmented control's well is 7px around 5px segments. Lists, notes and inline confirms are 8px; popovers and toasts 10px. Game art and thumbnails 5px, tags and keycaps 4px. Fully round for avatars, dots, badges, chips, counts, the switch and scrollbars. Checkboxes are not drawn boxes but Material Symbols glyph-icons (outline, filled check, filled indeterminate), 18px. Selection and "current" are tonal grounds, the current tab a 2px white underline.

## Components

### Buttons
Quiet until pointed at; one red.
- **Shape:** 6px corners, 32px tall (small 28px, large 40px), 13px at 500.
- **Primary:** Signal Red with Ember Black text and icon, 600, 0 14px; lit red on hover. Used once per view.
- **Secondary:** Raised, Raised 2 on hover; the default for every other action, including a group's Launch (92px wide, play icon).
- **Ghost:** no ground until hover (Raised). Toolbar actions for the selection are ghost buttons with a leading icon and a trailing chevron where they open a menu.
- **Danger:** Fault Coral with Ember Black text, only inside an inline confirm.
- **Icon button:** 32px square (28px small), Secondary Grey icon; Raised with white icon on hover; Raised 2 with white when toggled.
- **Quiet button:** reads as its words until hovered, then a Raised ground; used for the row's performance select and macro picker.
- **Pressed / disabled:** pressed adds a 14% black overlay; disabled drops to 45% opacity.
- **Focus:** a 2px Signal Red outline, 1px out, on every focusable control.

### Checkboxes and Switches
- **Checkbox:** a Material Symbols icon set through `-gtk-icon-source` (`rm-checkbox-off`, `-on`, `-mixed`), 18px. Dim Grey empty, Secondary Grey on hover, Readout White checked or indeterminate.
- **Switch:** 32 by 18px pill; Raised 2 off with a grey knob, Readout White on with a black knob. No icons inside.

### Chips and Badges
- **Badge:** 22px pill on Raised, 12px at 500, a 7px dot in the status colour plus a word.
- **Chip:** 24px pill on Raised; Raised 2 on hover.
- **Count:** a pill on Raised in Secondary Grey; the sidebar's Needs attention count turns coral on a 16% coral wash when it is not zero.

### Cards / Containers
- **Lists:** Panel, 8px, an inset 1px Hairline ring, rows split by hairlines, 40px rows.
- **Notes:** Raised, 8px, 10px 12px padding.
- **Inline confirm:** an 8% coral wash in Panel with a 30% coral ring, Cancel (secondary) and the destructive verb (danger) at its end. Confirms appear in place, in the row, not in dialogs.

### Inputs / Fields
- **Style:** Raised ground, no visible border, 6px, 32px, 13px; Raised 2 on hover.
- **Focus:** a Hairline, Strong border plus the 2px Signal Red outline.
- **Error:** the border turns Fault Coral.
- **Search:** the header's search field, Raised, with a Dim Grey icon and placeholder.
- **Select:** a dropdown on the input ground with a chevron, 500 weight.

### Segmented Control
A Control-room Black well with an inset Hairline ring and 2px padding; segments 26px, Secondary Grey. The chosen segment is Raised 2 with white text and the segment lift. Used for Performance (Low, Medium, High, Max).

### Navigation
- **Header bar:** Side Layer, 52px: the mark, "Stacked" and a Dim Grey status line ("All idle"), a centred search, then add, refresh and the menu as icon buttons.
- **Sidebar:** Side Layer, 240px. Section heads in Dim Grey; 32px items with a Dim Grey leading icon and a right-aligned tabular count. The chosen item is Raised with its icon turned white. Groups show their game icon, macros a play or stop icon (green while playing).
- **Tabs:** text tabs in Dim Grey, the current one white with a 2px white inset underline; the details pane's tabs are full head height (52px).
- **Status bar:** Side Layer, 30px, 12px Dim Grey text: the multi-client risk line at the left, the last activity with its state-coloured icon, and the Activity drawer toggle (its chevron turns 180 degrees when open).

### Accounts Table (signature)
- **Head:** 34px, 12px at 500 in Dim Grey, sentence case.
- **Account row:** 40px on Control-room Black: checkbox, avatar, name (14px 500) and @username (13px Dim Grey), a status dot and word, uptime ("7 d ago", "2 h 14 m"), CPU and memory right-aligned, the macro (name and round or step, or a quiet "Add macro"), the quiet performance select (level at 600, FPS in grey, an amber marker when the running client differs), then play or stop and more. The main account is pinned at the top with a star.
- **States:** hover and selection are faint tints; the row shown in the details pane is Raised; dragging drops to 40% opacity with a 1px Secondary Grey insertion line.
- **Group row:** 42px on the Side Layer: checkbox, fold chevron (rotates -90 degrees when folded), game icon, name at 600, "0 of 13 running", the game picker spanning the reading columns, CPU and memory totals, then a secondary Launch or Stop.
- **Needs attention:** rows at the top of the table, each with its reason and its fix as a secondary button.

### Details Pane
Panel under the table, a hairline above. A 52px head with avatar, name (15px 600), "@user · group" and a status dot and word, the tabs (Controls, Macro steps, Log) and a fold button. Controls is a grid of fields, each a 12px Dim Grey label over its control and 12px help: Performance (segmented, what it does, "A change applies the next time it starts"), Macro (select, Run, Edit), Window (Hide window, Macro-ready switch), Session (signed-in state with icon, Check now, Move to).

## Do's and Don'ts

### Do:
- **Do** keep Signal Red (#ff3d2e) for the one primary action and the focus ring.
- **Do** draw "on" in white: checked checkboxes, switches, the chosen segment's label, the current tab's underline.
- **Do** put state colour only in 7 to 8px dots and icons, always beside a word.
- **Do** separate regions and rows with 1px Hairlines on the tonal blacks.
- **Do** use 6px corners on every control and Material Symbols Rounded 400 for every icon, embedded as SVG and drawn in the text colour.
- **Do** write every label in sentence case and every compared number in tabular numerals.
- **Do** let only the account column expand in the table; every other column keeps its width and leaves in order as the window narrows.
- **Do** give every surface that gets a background its text colour too, so the desktop's palette never shows through.

### Don't:
- **Don't** colour a group's Launch, Run or any second action red; they are secondary buttons.
- **Don't** use Signal Red for errors or expired sessions; that is Fault Coral.
- **Don't** add coloured edge stripes, coloured row grounds or tinted cards to show state.
- **Don't** put a shadow on anything that rests; Float is for popovers, toasts and the drawer.
- **Don't** upper-case or letter-space labels, heads or columns.
- **Don't** depend on the desktop's fonts or icon theme; Inter, JetBrains Mono and the icons ship with the app.
- **Don't** add a light theme; the window is dark only.
