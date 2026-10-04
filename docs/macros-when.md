# When steps

A `when` line waits for something to show in the client, and the `do`
lines under it play the moment it does.

```
tap w
wait 2
when image image1 812 40
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
`image imageN X Y`, where X Y is the area's top-left corner in the
client window's coordinates (the same ones a Click uses). Picking again
makes a new image and never overwrites one another macro might use.

## What it can wait for

| Line | Plays when |
| --- | --- |
| `when image NAME X Y [PERCENT%]` | the image is at X Y again, within 4 pixels, and at least PERCENT alike (90% by default) |
| `when not image NAME X Y [PERCENT%]` | the image was there and has gone |
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
size it was when you picked them. The image is looked for where you picked
it, give or take 4 pixels. It is not searched for across the whole window.

The frame is copied out by cage, the macro-ready window's own compositor,
the way a screenshot tool asks for one. Nothing reads from or reaches into
the Roblox client. A client launched by an earlier version, before
macro-ready windows could be hidden, has to be launched again for its
whens to see while its window is hidden.
