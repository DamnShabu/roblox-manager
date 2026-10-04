# When steps

A `when` line waits for something to show in the client, and the `do`
lines under it play the moment it does.

```
tap w
wait 2
when image image1
do tap e
do wait 0.5
do click 400 300
when not color 960 30 #ff3030 10
do press shift
```

## Picking an image

Add a **When** step in the editor and press the image button on its row.
If more than one macro-ready client is running, pick one. Drag over what
to wait for. When you let go, that area of the client is copied and kept
as `~/.local/share/rbxmgr/macro-images/imageN.png`, and the row reads
`image imageN`: looked for anywhere in the window. Picking again makes a
new image and never overwrites one another macro might use; a rectangle
the row already named (`in X Y W H`) is kept.

## What it can wait for

| Line | Plays when |
| --- | --- |
| `when image NAME [PERCENT%]` | the image shows anywhere in the window, at least PERCENT alike (90% by default) |
| `when image NAME in X Y W H [PERCENT%]` | the image shows anywhere in that rectangle: corner X Y, W wide and H high |
| `when image NAME X Y [PERCENT%]` | the image shows with its corner at X Y, within 4 pixels |
| `when not image NAME ...` | the image was there and has gone |
| `when color X Y #RRGGBB [WITHIN]` | the pixel at X Y is that colour, each channel within WITHIN (24 by default) |
| `when not color X Y #RRGGBB [WITHIN]` | the pixel was that colour and no longer is |

A `do` line can be any step but Repeat, Start, Stagger or a Timeline.
`do exit` ends the round the macro is in, straight away: the when's
remaining steps are skipped, everything the round holds is let go of, and
the next round starts (or the macro stops, after its last).

## How it plays

The client's frame is looked at twenty times a second, so a when plays
within about a twentieth of a second of what it waits for appearing. The
macro's own steps pause while it plays and pick up after it. Keys they
were holding stay down, and a wait they were in counts on through it, so
a long when shortens the wait it interrupted. If two whens see at once,
the second plays straight after the first.

A when plays once each time what it waits for appears. It plays again only
after it has gone and come back. One that is already showing when the
macro starts plays straight away.

A macro can be nothing but whens. It then watches until you stop it.

## What it cannot do

Points and images are in the window's own pixels, so keep the window the
size it was when you picked them: an image is matched at the size it was
picked, not scaled. A look at the whole window takes a few milliseconds
(a copy of a 1280×720 window measured 5–11 ms, the search 1.5–3 ms for
an image 24 pixels or more across, about 9 ms for a 12-pixel one). A
rectangle or a point makes it cheaper. Where the image was last found is
tried first, so one that stays put costs almost nothing.

The frame is copied out by cage, the macro-ready window's own compositor,
the way a screenshot tool asks for one. Nothing reads from or reaches into
the Roblox client. A client launched by an earlier version, before
macro-ready windows could be hidden, has to be launched again for its
whens to see while its window is hidden.
